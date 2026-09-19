//! IMAP sync (Milestone 1): connect, LIST folders with role mapping,
//! SELECT + UID FETCH into SQLite, flag push, expunge handling.
//!
//! Transport: implicit TLS (port 993) or STARTTLS (port 143). Plaintext is
//! refused unless the account explicitly opts in (see AGENT.md security rules).

use std::collections::HashSet;
use std::net::TcpStream;

use imap::types::{Flag, NameAttribute};
use imap_proto::types::Capability;
use native_tls::{TlsConnector, TlsStream};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Folder, FolderRole, Message, NewAttachment, NewMessage};
use crate::store::{accounts, contacts, folders, messages, settings};
use crate::sync::traits::{SyncProvider, SyncReport};

type TlsSession = imap::Session<TlsStream<TcpStream>>;

/// How many UIDs per FETCH round-trip.
const FETCH_CHUNK: usize = 100;

/// Newest-N window for a full folder sync (matches `feed::FEED_LIMIT`).
/// Bounding the fetch keeps massive mailboxes fast: the list only shows 200,
/// so downloading 10k full RFC822 bodies on every ⟳ is pure waste.
/// Older mail backfills on demand (per-folder sync / scroll pagination).
pub const FULL_SYNC_WINDOW: usize = 200;
/// Newest-N window for background auto-sync of non-Inbox folders.
/// Non-default folders never auto-sync all mail — they refresh flags + the
/// newest few headers for the sidebar pills, and fetch fully when opened.
pub const QUICK_SYNC_WINDOW: usize = 50;
/// One "load older" batch: how many older mails a single button press pulls.
/// Matches the feed page so each press visibly grows the list by one page.
pub const OLDER_BATCH: usize = 200;

/// Max bytes stored per attachment (25 MiB). Larger parts are skipped with a
/// warning so one huge file cannot blow up the offline SQLite cache; the
/// message itself still syncs and `has_attachments` stays true.
pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
/// Max attachments stored per message (header + body safety bound).
pub const MAX_ATTACHMENTS_PER_MESSAGE: usize = 50;

/// Resolved IMAP endpoint for one account.
#[derive(Debug, Clone)]
pub struct ImapEndpoint {
    /// `host:port`.
    pub addr: String,
    /// `true` for implicit TLS (993 / `tls`).
    pub implicit_tls: bool,
    /// `true` for STARTTLS upgrade (typically 143). Mutually exclusive with
    /// [`Self::implicit_tls`]. Neither set means plaintext, which [`ImapSync::connect`]
    /// refuses.
    pub starttls: bool,
}

/// Derive the endpoint from account settings.
#[must_use]
pub fn endpoint_for(account: &crate::models::Account) -> ImapEndpoint {
    let sec = account.imap_security.trim().to_ascii_lowercase();
    let implicit_tls = match sec.as_str() {
        "starttls" | "plain" | "none" => false,
        "tls" => true,
        _ => account.imap_port == 993,
    };
    let starttls = match sec.as_str() {
        "starttls" => true,
        "tls" | "plain" | "none" => false,
        _ => account.imap_port == 143,
    };
    ImapEndpoint {
        addr: format!("{}:{}", account.imap_host, account.imap_port),
        implicit_tls,
        starttls,
    }
}

/// Whether a LISTED mailbox can hold messages (i.e. not `\Noselect` and not
/// a `\NonExistent` hierarchy placeholder).
#[must_use]
pub fn is_selectable(attributes: &[NameAttribute]) -> bool {
    let t = attr_text(attributes);
    !t.contains("noselect") && !t.contains("nonexistent")
}

/// Map a LISTED mailbox to a [`FolderRole`].
///
/// Prefers RFC 6154 SPECIAL-USE attributes, falls back to multilingual name
/// heuristics, keeps everything else as `Custom` (user IMAP folders included).
#[must_use]
pub fn map_folder_role(attributes: &[NameAttribute], name: &str) -> FolderRole {
    let attrs = attr_text(attributes);
    if attrs.contains("sent") {
        return FolderRole::Sent;
    }
    if attrs.contains("draft") {
        return FolderRole::Drafts;
    }
    if attrs.contains("trash") {
        return FolderRole::Trash;
    }
    if attrs.contains("junk") || attrs.contains("spam") {
        return FolderRole::Junk;
    }
    if attrs.contains("archive") {
        return FolderRole::Archive;
    }
    role_from_name(name)
}

/// Name-based role guess (used when SPECIAL-USE is absent).
///
/// Matches the last path segment exactly so names like `Cabin` / `Binders`
/// are not classified as Trash via a substring `bin`.
#[must_use]
pub fn role_from_name(name: &str) -> FolderRole {
    let lower = name.to_lowercase();
    let last = lower
        .rsplit(['/', '.', '\\'])
        .next()
        .unwrap_or(&lower)
        .trim();
    if last == "inbox" {
        return FolderRole::Inbox;
    }
    match last {
        "sent" | "gesendet" | "sent mail" | "sent items" | "sent-mail" => FolderRole::Sent,
        "draft" | "drafts" | "entwurf" | "entwürfe" | "entwurfe" => FolderRole::Drafts,
        "trash" | "deleted" | "deleted items" | "papierkorb" | "gelöscht" | "geloscht" | "bin" => {
            FolderRole::Trash
        }
        "junk" | "spam" | "junk e-mail" | "junk email" | "junk-e-mail" => FolderRole::Junk,
        "archive" | "archiv" => FolderRole::Archive,
        _ => FolderRole::Custom,
    }
}

fn attr_text(attributes: &[NameAttribute]) -> String {
    attributes
        .iter()
        .map(|a| format!("{a:?}"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Namespace prefixes reported by the server (RFC 2342): personal, other
/// users', and shared. Each entry is `(prefix, delimiter-or-None)`.
#[derive(Debug, Default, PartialEq, Eq)]
struct Namespaces {
    personal: Vec<(String, Option<String>)>,
    other: Vec<(String, Option<String>)>,
    shared: Vec<(String, Option<String>)>,
}

/// Parse a `* NAMESPACE ((...)...) ((...)...) ((...)...)` response into its
/// three prefix groups. Malformed input yields what parsed so far (possibly
/// empty) — never an error, since this is only a discovery hint.
fn parse_namespace_response(raw: &[u8]) -> Namespaces {
    let text = String::from_utf8_lossy(raw);
    let Some(start) = text.find("NAMESPACE") else {
        return Namespaces::default();
    };
    let bytes = text[start..].as_bytes();
    let mut pos = "NAMESPACE".len();
    let mut groups: Vec<Vec<(String, Option<String>)>> = Vec::new();
    while groups.len() < 3 {
        skip_ws(bytes, &mut pos);
        if bytes.get(pos) == Some(&b'N') && text[start + pos..].starts_with("NIL") {
            groups.push(Vec::new());
            pos += 3;
            continue;
        }
        if bytes.get(pos) != Some(&b'(') {
            break;
        }
        pos += 1; // outer '('
        let mut entries = Vec::new();
        loop {
            skip_ws(bytes, &mut pos);
            if bytes.get(pos) == Some(&b')') {
                pos += 1;
                break;
            }
            if bytes.get(pos) != Some(&b'(') {
                break;
            }
            pos += 1; // entry '('
            skip_ws(bytes, &mut pos);
            let prefix = parse_ns_string(bytes, &mut pos);
            skip_ws(bytes, &mut pos);
            let delim = parse_ns_string(bytes, &mut pos);
            skip_ws(bytes, &mut pos);
            if bytes.get(pos) == Some(&b')') {
                pos += 1;
            }
            match (prefix, delim) {
                (Some(Some(p)), Some(d)) => entries.push((p, d)),
                _ => break,
            }
        }
        groups.push(entries);
    }
    Namespaces {
        personal: groups.first().cloned().unwrap_or_default(),
        other: groups.get(1).cloned().unwrap_or_default(),
        shared: groups.get(2).cloned().unwrap_or_default(),
    }
}

fn skip_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\r' | b'\n') {
        *pos += 1;
    }
}

/// Parse a quoted string (with backslash escapes) or `NIL` (→ `None`).
fn parse_ns_string(bytes: &[u8], pos: &mut usize) -> Option<Option<String>> {
    if bytes.get(*pos) == Some(&b'"') {
        *pos += 1;
        let mut out = String::new();
        while *pos < bytes.len() {
            let c = bytes[*pos];
            if c == b'\\' && *pos + 1 < bytes.len() {
                *pos += 1;
                out.push(bytes[*pos] as char);
            } else if c == b'"' {
                *pos += 1;
                return Some(Some(out));
            } else {
                out.push(c as char);
            }
            *pos += 1;
        }
        return Some(Some(out));
    }
    if *pos + 3 <= bytes.len()
        && bytes[*pos..].starts_with(b"NIL")
        && bytes
            .get(*pos + 3)
            .is_none_or(|c| matches!(c, b' ' | b'\t' | b'\r' | b'\n' | b')'))
    {
        *pos += 3;
        return Some(None);
    }
    None
}

/// IMAP sync session. Construct with [`ImapSync::new`], then [`ImapSync::connect`].
pub struct ImapSync {
    host: String,
    port: u16,
    username: String,
    implicit_tls: bool,
    starttls: bool,
    /// Login password, memory-only (never logged, never stored — see AGENT.md:
    /// secrets live in the keyring or memory). Kept so the session can
    /// re-establish itself after a protocol desync without another keyring
    /// round-trip; cleared on [`ImapSync::disconnect`].
    password: Option<String>,
    session: Option<TlsSession>,
    /// NAMESPACE probe outcome: once a server proves it answers NAMESPACE
    /// with a shape our parser cannot model, stop asking on this session.
    /// Asking anyway costs a full reconnect per sync (the unread tagged
    /// completion desyncs the stream) for zero namespaces in return.
    /// Reset on every fresh connect; a replacement session re-probes once.
    skip_namespaces: bool,
}

impl ImapSync {
    /// Build from account settings (password supplied at [`ImapSync::connect`]).
    #[must_use]
    pub fn new(account: &crate::models::Account) -> Self {
        let ep = endpoint_for(account);
        Self {
            host: account.imap_host.clone(),
            port: account.imap_port,
            username: account.imap_username.clone(),
            implicit_tls: ep.implicit_tls,
            starttls: ep.starttls,
            password: None,
            session: None,
            skip_namespaces: false,
        }
    }

    /// Dial + LOGIN. Password comes from the OS keyring (or test env).
    pub fn connect(&mut self, password: &str) -> Result<()> {
        if self.session.is_some() {
            return Ok(());
        }
        if !self.implicit_tls && !self.starttls {
            return Err(StoreError::InvalidInput(format!(
                "refusing plaintext IMAP for {} (TLS or STARTTLS required)",
                self.host
            )));
        }
        let mode = if self.implicit_tls { "tls" } else { "starttls" };
        log::info!("imap: connecting to {}:{} ({mode})", self.host, self.port);
        let tls = TlsConnector::builder().build()?;
        let client = if self.implicit_tls {
            imap::connect((self.host.as_str(), self.port), &self.host, &tls)?
        } else {
            imap::connect_starttls((self.host.as_str(), self.port), &self.host, &tls)?
        };
        let mut session = client
            .login(self.username.as_str(), password)
            .map_err(|(e, _)| StoreError::Imap(e))?;
        self.password = Some(password.to_string());
        // Raw protocol trace for diagnosing quirky servers (e.g. Tobit
        // David): set MAILCLIENT_IMAP_DEBUG=1 to eprint every C:/S: line.
        // WARNING: this includes the LOGIN password and message bodies —
        // redact before sharing any captured log.
        if std::env::var("MAILCLIENT_IMAP_DEBUG").is_ok() {
            session.debug = true;
            log::warn!("imap protocol debug on: raw traffic on stderr, redact before sharing");
        }
        log::info!("imap: logged in as {}", self.username);
        self.session = Some(session);
        // Fresh stream: re-probe NAMESPACE once (a replacement session may
        // talk to a fixed server, or a different backend behind a proxy).
        self.skip_namespaces = false;
        Ok(())
    }

    /// LOGOUT (best effort). Also forgets the stored password.
    pub fn disconnect(&mut self) {
        if let Some(mut s) = self.session.take() {
            let _ = s.logout();
        }
        self.password = None;
    }

    /// Drop the connection without LOGOUT and re-establish it with the stored
    /// password. Heals a desynced stream (e.g. a stale tagged response our
    /// parser couldn't consume past) — LOGOUT itself would trip over the same
    /// stale bytes, so it is deliberately skipped here.
    pub fn reconnect(&mut self) -> Result<()> {
        let password = self.password.clone().ok_or_else(|| {
            StoreError::InvalidInput("no stored password for reconnect".to_string())
        })?;
        log::warn!("imap: reconnecting {} to resync the stream", self.host);
        self.session.take();
        self.connect(&password)
    }

    /// Liveness probe for pooled sessions: one NOOP round-trip. `false` =
    /// dead, half-closed, or never connected — the caller should drop this
    /// session and connect fresh rather than send real work into it.
    pub fn is_healthy(&mut self) -> bool {
        match self.session.as_mut() {
            Some(s) => s.noop().is_ok(),
            None => false,
        }
    }

    /// Server-side search backfill for the search UI: the local FTS index
    /// only covers synced mail, so when it runs thin the UI asks the server
    /// too. Runs `UID SEARCH TEXT` per token in every folder of the account,
    /// or in just one folder when `folder_scope` is set (the search UI's
    /// folder checkbox), intersects the per-token hits (AND, like FTS), and
    /// fetches full bodies only for UIDs missing locally (newest 50 per
    /// folder, 100 total). Fetched mail lands in SQLite + the FTS index through the normal
    /// upsert path, so a plain local re-query picks it up — and later syncs
    /// keep it like any cached mail. Only ASCII tokens go over the wire
    /// (IMAP SEARCH strings are ASCII unless UTF8=ACCEPT is negotiated,
    /// which we don't); the rest stays FTS-only.
    pub fn search_server_into_cache(
        &mut self,
        db: &Db,
        account_id: i64,
        tokens: &[String],
        folder_scope: Option<&str>,
    ) -> Result<ServerSearchReport> {
        const PER_FOLDER_CAP: usize = 50;
        const TOTAL_CAP: u64 = 100;
        let mut report = ServerSearchReport::default();
        let ascii: Vec<&str> = tokens
            .iter()
            .map(String::as_str)
            .filter(|t| t.is_ascii())
            .collect();
        if ascii.is_empty() {
            return Ok(report);
        }
        let account = accounts::get(db, account_id)?;
        let targets: Vec<_> = folders::list_by_account(db, account_id)?
            .into_iter()
            .filter(|f| folder_scope.is_none_or(|s| f.path == s))
            .collect();
        for folder in targets {
            if report.fetched >= TOTAL_CAP {
                break;
            }
            let session = self.session()?;
            if session.select(&folder.path).is_err() {
                log::debug!("search: cannot select {}", folder.path);
                continue;
            }
            report.folders_searched += 1;
            let mut hits: Option<HashSet<u32>> = None;
            let mut failed = false;
            for tok in &ascii {
                match session.uid_search(format!("TEXT \"{tok}\"")) {
                    Ok(set) => {
                        hits = Some(match hits {
                            Some(h) => h.intersection(&set).copied().collect(),
                            None => set,
                        });
                    }
                    Err(e) => {
                        log::debug!("search: {} TEXT query failed: {e}", folder.path);
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                continue;
            }
            // Newest first, metadata only — bytes stay server-side until an
            // explicit open/download, exactly like background sync.
            let local: HashSet<u32> = messages::list_uids(db, folder.id)?.into_iter().collect();
            let mut missing: Vec<u32> = hits
                .unwrap_or_default()
                .difference(&local)
                .copied()
                .collect();
            missing.sort_unstable_by(|a, b| b.cmp(a));
            missing.truncate(PER_FOLDER_CAP);
            for chunk in missing.chunks(FETCH_CHUNK) {
                if report.fetched >= TOTAL_CAP {
                    break;
                }
                let set = chunk
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                for msg in session.uid_fetch(set, "(UID FLAGS BODY.PEEK[])")?.iter() {
                    let uid = msg.uid.unwrap_or(0);
                    if uid == 0 {
                        continue;
                    }
                    let raw = msg.body().unwrap_or_default();
                    let (parsed, files) =
                        parse_to_new(account.id, folder.id, uid, msg.flags(), raw, false)?;
                    let id = messages::upsert(db, &parsed)?;
                    collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
                    store_attachment_meta(db, id, files);
                    report.fetched += 1;
                }
            }
        }
        Ok(report)
    }

    /// Server CAPABILITY names (RFC 3501 §7.2.1) for the settings About view.
    /// One cheap round-trip on the pooled session, no folder selected.
    /// Returns sorted, de-duplicated names (`IMAP4rev1`, `IDLE`, `MOVE`,
    /// `AUTH=PLAIN`, …).
    pub fn capabilities_list(&mut self) -> Result<Vec<String>> {
        let caps = self.session()?.capabilities()?;
        let mut out: Vec<String> = caps
            .iter()
            .map(|c| match c {
                Capability::Imap4rev1 => "IMAP4rev1".to_string(),
                Capability::Auth(mech) => format!("AUTH={mech}"),
                Capability::Atom(atom) => (*atom).to_string(),
            })
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    /// Ask the server for its namespaces (best effort — many servers don't
    /// implement RFC 2342, and groupware like Tobit David hides branches the
    /// login isn't entitled to; either way we just get fewer prefixes).
    ///
    /// A parse failure means the server sent a response shape our IMAP parser
    /// cannot model (proven: Tobit's `* NAMESPACE` line, which imap-proto 0.10
    /// has no type for). The tagged completion then stays unread in the socket
    /// buffer and the *next* command dies on the crate's tag assert — so on
    /// exactly this error the session reconnects itself before returning.
    /// BAD/NO answers are clean (their tagged line was consumed) and need nothing.
    fn query_namespaces(&mut self) -> Namespaces {
        // Probed unparseable before on this session: asking again would buy
        // another full reconnect for zero namespaces.
        if self.skip_namespaces {
            return Namespaces::default();
        }
        let raw = match self.session() {
            Ok(session) => match session.run_command_and_read_response("NAMESPACE") {
                Ok(raw) => raw,
                Err(imap::Error::Parse(_)) => {
                    log::warn!("imap: NAMESPACE response unparseable, reconnecting to resync");
                    if let Err(e) = self.reconnect() {
                        log::warn!("imap: reconnect failed: {e}");
                    }
                    // After the resync (connect() clears the flag for the
                    // fresh stream): never ask again on this session.
                    self.skip_namespaces = true;
                    return Namespaces::default();
                }
                Err(e) => {
                    log::debug!("imap: NAMESPACE unsupported, skipping: {e}");
                    return Namespaces::default();
                }
            },
            Err(e) => {
                log::debug!("imap: no session for NAMESPACE query: {e}");
                return Namespaces::default();
            }
        };
        parse_namespace_response(&raw)
    }

    /// APPEND raw MIME bytes to a folder (used for Sent copies), marked `\Seen`.
    pub fn append_to_folder(&mut self, folder_path: &str, raw: &[u8]) -> Result<()> {
        self.session()?
            .append_with_flags(folder_path, raw, &[Flag::Seen])?;
        Ok(())
    }

    /// APPEND raw MIME bytes as a server-side draft. Drafts are deliberately
    /// not marked seen: the `\Draft` flag is what makes providers keep them
    /// out of normal send flows.
    pub fn append_draft(&mut self, folder_path: &str, raw: &[u8]) -> Result<()> {
        self.session()?
            .append_with_flags(folder_path, raw, &[Flag::Draft])?;
        Ok(())
    }

    /// Move one message to the account's Trash folder -- what "delete" means
    /// in a mail client, with two exceptions that destroy immediately:
    ///
    /// - the message is already in Trash (deleting from Trash is permanent),
    /// - the message is spam (filing junk into Trash just moves garbage
    ///   around — it is destroyed instead).
    ///
    /// Callers get [`TrashOutcome::Expunged`] for both; otherwise the
    /// destination folder path. Prefers `UID MOVE` (RFC 6851) and falls back
    /// to `COPY` + `\Deleted` + `EXPUNGE` on servers without it.
    pub fn trash_message(&mut self, db: &Db, message_id: i64) -> Result<TrashOutcome> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;

        // Spam never touches Trash; Trash never keeps a second copy of itself.
        if folder.role == FolderRole::Junk {
            log::info!("imap: destroying spam directly (uid {})", message.uid);
            self.delete_message(db, message_id)?;
            return Ok(TrashOutcome::Expunged);
        }
        let trash = folders::list_by_account(db, message.account_id)?
            .into_iter()
            .find(|f| f.role == FolderRole::Trash);

        // Already in Trash, or no Trash at all: the only remaining meaning of
        // "delete" is destroying it, and the caller is told so.
        let Some(trash) = trash.filter(|t| t.id != folder.id) else {
            self.delete_message(db, message_id)?;
            return Ok(TrashOutcome::Expunged);
        };

        self.move_message_to(db, message_id, &trash.path)?;
        // The server copy now lives in Trash; the local row belongs to the
        // source folder and is gone from it. Syncing Trash pulls it back.
        Ok(TrashOutcome::Moved(trash.path))
    }

    /// Move one message to the account's Archive folder — the one-click
    /// "archive" action. Creates the Archive folder server-side when the
    /// account has none, so the button always works. Archiving from inside
    /// Archive itself is a no-op reported as [`ArchiveOutcome::AlreadyThere`].
    pub fn archive_message(&mut self, db: &Db, message_id: i64) -> Result<ArchiveOutcome> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;

        let archive = match folders::list_by_account(db, message.account_id)?
            .into_iter()
            .find(|f| f.role == FolderRole::Archive)
        {
            Some(a) => a,
            None => {
                log::info!("imap: no Archive folder, creating one");
                let delim = folders::list_by_account(db, message.account_id)
                    .ok()
                    .and_then(|fs| fs.first().map(|f| f.delimiter.clone()))
                    .unwrap_or_else(|| "/".to_string());
                self.create_folder_path(db, message.account_id, "Archive", &delim)?
            }
        };
        if archive.id == folder.id {
            return Ok(ArchiveOutcome::AlreadyThere);
        }
        self.move_message_to(db, message_id, &archive.path)?;
        Ok(ArchiveOutcome::Moved(archive.path))
    }

    /// Move one message to an arbitrary folder of the same account — the
    /// "move to…" action. Moving into the folder it already lives in is a no-op
    /// reported as [`MoveOutcome::AlreadyThere`]. Subfolders work like any other
    /// path (their hierarchy separator is part of the stored path).
    pub fn move_to_folder(
        &mut self,
        db: &Db,
        message_id: i64,
        dest_id: i64,
    ) -> Result<MoveOutcome> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;
        let dest = folders::get(db, dest_id)?;
        if dest.account_id != message.account_id {
            return Err(StoreError::InvalidInput(
                "destination folder belongs to another account".to_string(),
            ));
        }
        if dest.id == folder.id {
            return Ok(MoveOutcome::AlreadyThere);
        }
        self.move_message_to(db, message_id, &dest.path)?;
        Ok(MoveOutcome::Moved(dest.path))
    }

    /// Server-side move of one message into `dest_path` (plus local row
    /// delete). Shared by trash and archive; prefers `UID MOVE`, falls back
    /// to `COPY` + `\Deleted` + `EXPUNGE`.
    ///
    /// Trash moves auto-mark `\Seen` first: deleting an unread message must
    /// not leave an unread copy in Trash. `MOVE`/`COPY` preserve flags, so
    /// the Seen bit carries over to the destination.
    fn move_message_to(&mut self, db: &Db, message_id: i64, dest_path: &str) -> Result<()> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;
        let dest_is_trash = folders::get_by_path(db, folder.account_id, dest_path)
            .map(|d| d.role == FolderRole::Trash)
            .unwrap_or(false);
        let has_move = self
            .session()?
            .capabilities()
            .map(|caps| caps.has_str("MOVE"))
            .unwrap_or(false);
        let session = self.session()?;
        session.select(&folder.path)?;
        let uid = message.uid.to_string();
        if dest_is_trash && !message.is_read {
            if let Err(e) = session.uid_store(&uid, "+FLAGS (\\Seen)") {
                log::warn!("imap: mark-seen before trash move failed: {e}");
            }
        }
        if has_move {
            session.uid_mv(&uid, dest_path)?;
        } else {
            session.uid_copy(&uid, dest_path)?;
            session.uid_store(&uid, "+FLAGS (\\Deleted)")?;
            session.expunge()?;
        }
        messages::delete(db, message_id)?;
        Ok(())
    }

    /// Create an IMAP mailbox (plus any missing parents) and register it
    /// locally via folder discovery. Returns the created [`Folder`].
    /// `delimiter` is the account's hierarchy separator (nested input like
    /// `Work/Client` uses it); missing parents are created first so one call
    /// creates the whole chain. An already-existing path is success, not an
    /// error — discovery simply returns it.
    pub fn create_folder_path(
        &mut self,
        db: &Db,
        account_id: i64,
        path: &str,
        delimiter: &str,
    ) -> Result<Folder> {
        let normalized = normalize_folder_path(path, delimiter)?;
        let mut prefix = String::new();
        for segment in normalized.split(delimiter) {
            if !prefix.is_empty() {
                prefix.push_str(delimiter);
            }
            prefix.push_str(segment);
            match self.session()?.create(&prefix) {
                Ok(()) => log::info!("imap: created folder {prefix}"),
                Err(e) if is_already_exists(&e) => log::debug!("imap: folder exists: {prefix}"),
                Err(e) => return Err(e.into()),
            }
        }
        self.sync_folders(db, account_id)?;
        folders::get_by_path(db, account_id, &normalized)
            .map_err(|_| StoreError::InvalidInput(format!("server did not list {normalized}")))
    }

    /// Permanently destroy one message server-side (`\Deleted` + expunge)
    /// and locally. No undo -- use [`Self::trash_message`] for the normal
    /// delete action.
    pub fn delete_message(&mut self, db: &Db, message_id: i64) -> Result<()> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;
        let session = self.session()?;
        session.select(&folder.path)?;
        session.uid_store(message.uid.to_string(), "+FLAGS (\\Deleted)")?;
        session.expunge()?;
        messages::delete(db, message_id)?;
        Ok(())
    }

    /// Bulk move cached UIDs of one folder into `dest_path` with a single
    /// SELECT + one UID MOVE (or COPY + `\Deleted` + EXPUNGE fallback), then
    /// drop the local source rows. Returns moved rows. `dest_path` must differ
    /// from the source folder's path (callers report "already here").
    ///
    /// Trash moves auto-mark `\Seen` first so deleted unread mail does not
    /// leave an unread copy in Trash.
    pub fn move_uids_to(
        &mut self,
        db: &Db,
        folder_id: i64,
        uids: &[u32],
        dest_path: &str,
    ) -> Result<u64> {
        let mut clean: Vec<u32> = uids.to_vec();
        clean.sort_unstable();
        clean.dedup();
        if clean.is_empty() {
            return Ok(0);
        }
        let folder = folders::get(db, folder_id)?;
        if folder.path == dest_path {
            return Ok(0);
        }
        let dest_is_trash = folders::get_by_path(db, folder.account_id, dest_path)
            .map(|d| d.role == FolderRole::Trash)
            .unwrap_or(false);
        let set = clean
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let has_move = self
            .session()?
            .capabilities()
            .map(|caps| caps.has_str("MOVE"))
            .unwrap_or(false);
        let session = self.session()?;
        session.select(&folder.path)?;
        if dest_is_trash {
            if let Err(e) = session.uid_store(&set, "+FLAGS (\\Seen)") {
                log::warn!("imap: mark-seen before bulk trash move failed: {e}");
            }
        }
        if has_move {
            session.uid_mv(&set, dest_path)?;
        } else {
            session.uid_copy(&set, dest_path)?;
            session.uid_store(&set, "+FLAGS (\\Deleted)")?;
            session.expunge()?;
        }
        let mut moved = 0u64;
        for uid in &clean {
            if messages::delete_by_uid(db, folder_id, *uid)? {
                moved += 1;
            }
        }
        Ok(moved)
    }

    /// Bulk permanent destroy of cached UIDs in one folder: a single SELECT +
    /// one UID STORE + EXPUNGE, then local deletes. Returns destroyed rows.
    pub fn purge_uids(&mut self, db: &Db, folder_id: i64, uids: &[u32]) -> Result<u64> {
        let mut clean: Vec<u32> = uids.to_vec();
        clean.sort_unstable();
        clean.dedup();
        if clean.is_empty() {
            return Ok(0);
        }
        let folder = folders::get(db, folder_id)?;
        let set = clean
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let session = self.session()?;
        session.select(&folder.path)?;
        session.uid_store(&set, "+FLAGS (\\Deleted)")?;
        session.expunge()?;
        let mut gone = 0u64;
        for uid in &clean {
            if messages::delete_by_uid(db, folder_id, *uid)? {
                gone += 1;
            }
        }
        Ok(gone)
    }

    /// Windowed folder sync: only the newest `window` server UIDs cost
    /// network (flag refresh + full RFC822 fetch). The UID SEARCH pages
    /// backwards to UID 1 when the range is sparse, so expunge diffing is a
    /// full local diff in the common case. `None` = all UIDs (used only by
    /// explicit tests — production callers pass `FULL_SYNC_WINDOW` /
    /// `QUICK_SYNC_WINDOW`).
    pub fn sync_folder_window(
        &mut self,
        db: &Db,
        folder_id: i64,
        window: Option<usize>,
    ) -> Result<SyncReport> {
        let folder = folders::get(db, folder_id)?;
        let account = accounts::get(db, folder.account_id)?;
        let session = self.session()?;

        let mb = session.select(&folder.path)?;
        log::info!(
            "imap: SELECT {} ({} mails, uid_next {:?})",
            folder.path,
            mb.exists,
            mb.uid_next
        );

        // UIDVALIDITY change => server-side rebuild, drop local copies.
        if let Some(validity) = mb.uid_validity {
            if folder.uid_validity.is_some_and(|v| v != validity) {
                log::warn!("imap: UIDVALIDITY changed for {} — resyncing", folder.path);
                messages::delete_by_folder(db, folder_id)?;
            }
        }

        let local_uids: HashSet<u32> = messages::list_uids(db, folder_id)?.into_iter().collect();
        let (searched_uids, search_lo) = search_recent_uids(session, window, mb.uid_next)?;

        // Newest-N relevance window: UIDs grow monotonically, so the largest
        // N are the newest. Everything outside costs no network.
        let relevant: Option<HashSet<u32>> = window.map(|n| {
            let mut sorted: Vec<u32> = searched_uids.iter().copied().collect();
            sorted.sort_unstable();
            let skip = sorted.len().saturating_sub(n);
            sorted.into_iter().skip(skip).collect()
        });
        let in_window = |uid: &u32| relevant.as_ref().is_none_or(|r| r.contains(uid));
        let server_uids = searched_uids;

        // 1. Flag refresh for messages we already have (windowed).
        let existing: Vec<u32> = server_uids
            .intersection(&local_uids)
            .copied()
            .filter(in_window)
            .collect();
        for chunk in existing.chunks(FETCH_CHUNK) {
            let set = chunk
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            for msg in session.uid_fetch(set, "(UID FLAGS)")?.iter() {
                if let Some(uid) = msg.uid {
                    let (read, starred, draft) = flag_state(msg.flags());
                    // A draft you wrote is not "unread" — keep the flag
                    // refresh from lighting up Drafts rows and pills.
                    messages::set_flags_by_uid(
                        db,
                        account.id,
                        folder_id,
                        uid,
                        read || draft,
                        starred,
                        draft,
                    )?;
                }
            }
        }

        // 2. Full fetch of new messages (windowed). BODY.PEEK[] is
        // mandatory here: a plain RFC822/BODY[] fetch implicitly sets
        // \Seen on most servers, which would mark every synced mail as
        // read behind the user's back (and silently kill unread counts
        // and new-mail notifications).
        let mut fetched = 0u64;
        let missing: Vec<u32> = server_uids
            .difference(&local_uids)
            .copied()
            .filter(in_window)
            .collect();
        // Fetch oldest-first within the window so the DB fills chronologically.
        let mut missing = missing;
        missing.sort_unstable();
        for chunk in missing.chunks(FETCH_CHUNK) {
            let set = chunk
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            for msg in session.uid_fetch(set, "(UID FLAGS BODY.PEEK[])")?.iter() {
                let uid = msg.uid.unwrap_or(0);
                if uid == 0 {
                    continue;
                }
                let raw = msg.body().unwrap_or_default();
                let (parsed, files) =
                    parse_to_new(account.id, folder_id, uid, msg.flags(), raw, false)?;
                let id = messages::upsert(db, &parsed)?;
                collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
                // Metadata only: attachment bytes stay on the server until the
                // user explicitly downloads a file.
                store_attachment_meta(db, id, files);
                fetched += 1;
            }
        }
        if let Some(n) = window {
            let skipped =
                (mb.exists as usize).saturating_sub(relevant.map(|r| r.len()).unwrap_or(0));
            if skipped > 0 {
                log::info!(
                    "imap: {} skipped {} old mails outside window {n}",
                    folder.path,
                    skipped
                );
            }
        }

        // 3. Expunge locally what the searched UID range no longer has.
        // UIDs below `search_lo` were never asked about, so they stay cached.
        let mut expunged = 0u64;
        for uid in &local_uids {
            if *uid >= search_lo && !server_uids.contains(uid) {
                messages::delete_by_uid(db, folder_id, *uid)?;
                expunged += 1;
            }
        }

        let validity = mb.uid_validity.unwrap_or(folder.uid_validity.unwrap_or(0));
        let uid_next = mb.uid_next.unwrap_or(folder.uid_next.unwrap_or(0));
        folders::set_sync_state(db, folder_id, validity, uid_next, u64::from(mb.exists))?;

        Ok(SyncReport {
            fetched,
            expunged,
            folders: 0,
        })
    }

    /// Fetch the next older batch below the smallest locally cached UID.
    ///
    /// The "load older messages" button path: a bounded `UID SEARCH` just
    /// below the smallest local UID, then a windowed `RFC822` fetch for up
    /// to `batch` missing older UIDs. Empty folders fall back to a normal
    /// windowed sync; a UIDVALIDITY change resyncs first so the window math
    /// stays valid. Returns `fetched == 0` when the cache already reaches
    /// the oldest server mail ("caught up").
    pub fn sync_older(&mut self, db: &Db, folder_id: i64, batch: usize) -> Result<SyncReport> {
        let folder = folders::get(db, folder_id)?;
        let account = accounts::get(db, folder.account_id)?;
        let session = self.session()?;

        let mb = session.select(&folder.path)?;
        if let Some(validity) = mb.uid_validity {
            if folder.uid_validity.is_some_and(|v| v != validity) {
                log::warn!(
                    "imap: UIDVALIDITY changed for {} — resyncing before older fetch",
                    folder.path
                );
                messages::delete_by_folder(db, folder_id)?;
                return self.sync_folder_window(db, folder_id, Some(FULL_SYNC_WINDOW));
            }
        }
        let validity = mb.uid_validity.unwrap_or(folder.uid_validity.unwrap_or(0));
        let uid_next = mb.uid_next.unwrap_or(folder.uid_next.unwrap_or(0));

        let Some(min_local) = messages::min_uid(db, folder_id)? else {
            // Nothing cached: a normal windowed sync is the first batch.
            let r = self.sync_folder_window(db, folder_id, Some(FULL_SYNC_WINDOW))?;
            return Ok(r);
        };
        if min_local <= 1 {
            folders::set_sync_state(db, folder_id, validity, uid_next, u64::from(mb.exists))?;
            return Ok(SyncReport::default());
        }
        let local_uids: HashSet<u32> = messages::list_uids(db, folder_id)?.into_iter().collect();
        let found = search_uids_below(session, min_local, batch)?;
        // Older than everything we have, newest-first within the batch so the
        // list extends contiguously backwards.
        let mut older: Vec<u32> = found.difference(&local_uids).copied().collect();
        older.sort_unstable_by(|a, b| b.cmp(a));
        older.truncate(batch);

        let mut fetched = 0u64;
        // Ascending fetch order keeps DB fill chronological within the batch.
        older.sort_unstable();
        for chunk in older.chunks(FETCH_CHUNK) {
            let set = chunk
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(",");
            for msg in session.uid_fetch(set, "(UID FLAGS BODY.PEEK[])")?.iter() {
                let uid = msg.uid.unwrap_or(0);
                if uid == 0 {
                    continue;
                }
                let raw = msg.body().unwrap_or_default();
                let (parsed, files) =
                    parse_to_new(account.id, folder_id, uid, msg.flags(), raw, false)?;
                let id = messages::upsert(db, &parsed)?;
                collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
                // Metadata only — see the windowed sync above.
                store_attachment_meta(db, id, files);
                fetched += 1;
            }
        }
        folders::set_sync_state(db, folder_id, validity, uid_next, u64::from(mb.exists))?;
        log::info!("imap: {} older batch: +{fetched}", folder.path);
        Ok(SyncReport {
            fetched,
            expunged: 0,
            folders: 0,
        })
    }

    /// Download one message's attachments on explicit user request (Save /
    /// Download click). Re-fetches the full body with PEEK (never implicitly
    /// marks `\Seen`), stores every part with bytes, and refreshes only the
    /// `has_attachments` flag — read/star state is never touched. Returns
    /// the number of stored files.
    pub fn fetch_attachments(&mut self, db: &Db, message_id: i64) -> Result<u64> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;
        let account = accounts::get(db, folder.account_id)?;
        let session = self.session()?;
        let started = std::time::Instant::now();
        session.select(&folder.path)?;
        log::info!("imap: select {} took {:?}", folder.path, started.elapsed());
        let started = std::time::Instant::now();
        let fetched = session.uid_fetch(message.uid.to_string(), "(UID FLAGS BODY.PEEK[])")?;
        log::info!(
            "imap: attachment fetch for uid {} took {:?}",
            message.uid,
            started.elapsed()
        );
        let mut stored = 0u64;
        let mut seen = false;
        for msg in fetched.iter() {
            let raw = msg.body().unwrap_or_default();
            let (_, files) =
                parse_to_new(account.id, folder.id, message.uid, msg.flags(), raw, true)?;
            stored = files.len() as u64;
            store_attachments(db, message_id, files)?;
            seen = true;
        }
        if !seen {
            return Err(StoreError::InvalidInput(format!(
                "message uid {} no longer on server",
                message.uid
            )));
        }
        messages::set_has_attachments(db, message_id, stored > 0)?;
        log::info!("imap: downloaded {stored} attachment(s) for message {message_id}");
        Ok(stored)
    }

    fn session(&mut self) -> Result<&mut TlsSession> {
        self.session.as_mut().ok_or_else(|| {
            StoreError::InvalidInput("not connected: call connect() first".to_string())
        })
    }
}

/// What [`ImapSync::trash_message`] actually did, so the UI can say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashOutcome {
    /// Moved to this folder path.
    Moved(String),
    /// Destroyed: it was spam, already in Trash, or the account has no Trash.
    Expunged,
}

/// What [`ImapSync::archive_message`] actually did, so the UI can say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveOutcome {
    /// Moved to this folder path.
    Moved(String),
    /// Already in Archive: nothing to do.
    AlreadyThere,
}

/// What [`ImapSync::move_to_folder`] actually did, so the UI can say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveOutcome {
    /// Moved to this folder path.
    Moved(String),
    /// Already in that folder: nothing to do.
    AlreadyThere,
}

/// Outcome of [`ImapSync::search_server_into_cache`], so the UI can say so.
#[derive(Debug, Default)]
pub struct ServerSearchReport {
    /// Folders successfully SELECTed + SEARCHed.
    pub folders_searched: usize,
    /// Full bodies fetched into the cache (bounded).
    pub fetched: u64,
}

/// Validate + normalize a user-typed folder path: trims whitespace, maps `/`
/// separators onto the account's hierarchy `delimiter`, rejects empties,
/// empty segments (`a//b`), and the LIST wildcards `*`/`%` (legal in theory,
/// but they would corrupt our own subtree discovery patterns).
pub fn normalize_folder_path(input: &str, delimiter: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(StoreError::InvalidInput("folder name is empty".to_string()));
    }
    if trimmed.contains('*') || trimmed.contains('%') {
        return Err(StoreError::InvalidInput(
            "folder names may not contain * or %".to_string(),
        ));
    }
    let unified = if delimiter != "/" {
        trimmed.replace('/', delimiter)
    } else {
        trimmed.to_string()
    };
    let segments: Vec<&str> = unified.split(delimiter).map(str::trim).collect();
    if segments.iter().any(|s| s.is_empty()) {
        return Err(StoreError::InvalidInput(
            "folder names may not be empty or contain empty levels".to_string(),
        ));
    }
    if segments.iter().any(|s| s.chars().any(char::is_control)) {
        return Err(StoreError::InvalidInput(
            "folder names may not contain control characters".to_string(),
        ));
    }
    Ok(segments.join(delimiter))
}

/// Best-effort "mailbox already exists" detection for CREATE races: servers
/// word it differently (`ALREADYEXISTS`, `already exists`, `exists`), so a
/// case-insensitive substring match beats an exact one.
fn is_already_exists(e: &imap::Error) -> bool {
    e.to_string()
        .to_ascii_lowercase()
        .contains("already exists")
        || e.to_string().to_ascii_lowercase().contains("alreadyexists")
}

impl SyncProvider for ImapSync {
    fn name(&self) -> &'static str {
        "imap"
    }

    fn sync_folders(&mut self, db: &Db, account_id: i64) -> Result<Vec<Folder>> {
        // Multi-pass discovery: a single `LIST "" "*"` misses folders on
        // servers with restricted LIST output or namespace gaps (users kept
        // seeing only the already-known folders, never e.g. Archive).
        // Pass 1 = full recursive LIST, pass 2 = LSUB merge (subscribed
        // folders some servers only report there), pass 3 = per-root subtree
        // LIST for namespace roots the bare "*" didn't expand (both the
        // reported delimiter and "." — Tobit David uses dotted prefixes),
        // pass 4 = LIST inside every NAMESPACE prefix (personal/other/shared).
        // First pass wins role mapping (it carries SPECIAL-USE); later passes
        // only add names we haven't seen. Auxiliary passes never fail sync.
        // Every first-seen entry is logged with its raw attributes so a
        // short list can be traced to exactly what the server reported.
        let mut seen: HashSet<String> = HashSet::new();
        let mut discovered: Vec<(String, String, FolderRole)> = Vec::new();
        let mut consider =
            |name: &str, delimiter: &str, role: FolderRole, attrs: &str, pass: &str| {
                if seen.insert(name.to_string()) {
                    log::info!(
                        "imap: [{pass}] [{attrs}] delim={delimiter:?} {name} -> {}",
                        role.as_str()
                    );
                    discovered.push((name.to_string(), delimiter.to_string(), role));
                }
            };

        let mut list_count = 0usize;
        let mut lsub_count = 0usize;
        let mut subtree_count = 0usize;

        // Pass 1: LIST "" "*".
        let names = self.session()?.list(Some(""), Some("*"))?;
        for n in names.iter() {
            if !is_selectable(n.attributes()) {
                log::debug!("imap: skipping non-selectable {}", n.name());
                continue;
            }
            consider(
                n.name(),
                n.delimiter().unwrap_or("/"),
                map_folder_role(n.attributes(), n.name()),
                &attr_text(n.attributes()),
                "LIST",
            );
            list_count += 1;
        }

        // Pass 2: LSUB "" "*" (best effort).
        match self.session()?.lsub(Some(""), Some("*")) {
            Ok(subs) => {
                for n in subs.iter() {
                    if !is_selectable(n.attributes()) {
                        continue;
                    }
                    consider(
                        n.name(),
                        n.delimiter().unwrap_or("/"),
                        role_from_name(n.name()),
                        &attr_text(n.attributes()),
                        "LSUB",
                    );
                    lsub_count += 1;
                }
            }
            Err(e) => log::warn!("imap: LSUB failed, continuing with LIST results: {e}"),
        }

        // Pass 3: subtree LIST per top-level root (best effort, capped).
        // Both the reported delimiter and "." are tried: Tobit David serves
        // dotted hierarchies (INBOX.Archive) that a "/"-joined pattern misses.
        match self.session()?.list(Some(""), Some("%")) {
            Ok(roots) => {
                for root in roots.iter().take(64) {
                    let delim = root.delimiter().unwrap_or("/");
                    let base = root.name();
                    if base.is_empty() {
                        continue;
                    }
                    let join = |d: &str| {
                        if base.ends_with(d) {
                            format!("{base}*")
                        } else {
                            format!("{base}{d}*")
                        }
                    };
                    let mut patterns = vec![join(delim)];
                    if delim != "." {
                        patterns.push(join("."));
                    }
                    for pattern in patterns {
                        match self.session()?.list(Some(""), Some(pattern.as_str())) {
                            Ok(children) => {
                                for n in children.iter() {
                                    if !is_selectable(n.attributes()) {
                                        continue;
                                    }
                                    consider(
                                        n.name(),
                                        n.delimiter().unwrap_or("/"),
                                        map_folder_role(n.attributes(), n.name()),
                                        &attr_text(n.attributes()),
                                        "SUBTREE",
                                    );
                                    subtree_count += 1;
                                }
                            }
                            Err(e) => log::debug!("imap: subtree LIST {pattern} failed: {e}"),
                        }
                    }
                }
            }
            Err(e) => log::debug!("imap: root LIST failed, skipping subtree pass: {e}"),
        }

        // Pass 4: LIST inside every NAMESPACE prefix (best effort, capped).
        // Shared / other-users' branches live outside "" and never appear in
        // passes 1–3; the server tells us where via RFC 2342 (when it bothers).
        // The empty personal prefix is pass 1 again, so it is skipped.
        let ns = self.query_namespaces();
        let mut ns_count = 0usize;
        for prefix in ns
            .personal
            .iter()
            .chain(ns.other.iter())
            .chain(ns.shared.iter())
            .map(|(p, _)| p)
            .filter(|p| !p.is_empty())
            .take(12)
        {
            match self.session()?.list(Some(prefix.as_str()), Some("*")) {
                Ok(extra) => {
                    for n in extra.iter() {
                        if !is_selectable(n.attributes()) {
                            continue;
                        }
                        consider(
                            n.name(),
                            n.delimiter().unwrap_or("/"),
                            map_folder_role(n.attributes(), n.name()),
                            &attr_text(n.attributes()),
                            "NAMESPACE",
                        );
                        ns_count += 1;
                    }
                }
                Err(e) => log::debug!("imap: namespace LIST {prefix:?} failed: {e}"),
            }
        }

        log::info!(
            "imap: discovery LIST*={list_count} LSUB={lsub_count} subtrees={subtree_count} namespaces={ns_count} merged={}",
            discovered.len()
        );
        let mut out = Vec::new();
        for (path, delimiter, role) in &discovered {
            let id = folders::upsert(db, account_id, path, delimiter, *role)?;
            out.push(folders::get(db, id)?);
        }
        log::info!("imap: {} folders", out.len());
        Ok(out)
    }

    fn sync_folder(&mut self, db: &Db, folder_id: i64) -> Result<SyncReport> {
        self.sync_folder_window(db, folder_id, Some(FULL_SYNC_WINDOW))
    }

    fn push_flags(&mut self, db: &Db, message: &Message) -> Result<()> {
        let folder = folders::get(db, message.folder_id)?;
        let session = self.session()?;
        session.select(&folder.path)?;
        let uid = message.uid.to_string();
        let seen = if message.is_read { "+FLAGS" } else { "-FLAGS" };
        let flagged = if message.is_starred {
            "+FLAGS"
        } else {
            "-FLAGS"
        };
        session.uid_store(&uid, format!("{seen} (\\Seen)"))?;
        session.uid_store(&uid, format!("{flagged} (\\Flagged)"))?;
        Ok(())
    }
}

/// SEARCH the newest `window` UIDs without enumerating the whole mailbox.
///
/// Returns `(uids, search_lo)`: every UID in `search_lo..=top` was asked
/// about, so local rows at or above `search_lo` missing from `uids` are safe
/// to expunge (`1` = the whole mailbox was covered: a full diff).
fn search_recent_uids(
    session: &mut TlsSession,
    window: Option<usize>,
    uid_next: Option<u32>,
) -> Result<(HashSet<u32>, u32)> {
    let Some(n) = window else {
        return Ok((session.uid_search("ALL")?, 1));
    };
    let top = uid_next.unwrap_or(0).saturating_sub(1);
    if top == 0 {
        return Ok((session.uid_search("ALL")?, 1));
    }
    let span = (n as u32).saturating_mul(8).max(n as u32);
    search_paged(session, top, n, span)
}

/// SEARCH a bounded UID range just below `exclusive_hi` for an older-mail batch.
fn search_uids_below(
    session: &mut TlsSession,
    exclusive_hi: u32,
    batch: usize,
) -> Result<HashSet<u32>> {
    if exclusive_hi <= 1 {
        return Ok(HashSet::new());
    }
    let top = exclusive_hi - 1;
    let span = (batch as u32).saturating_mul(8).max(batch as u32);
    Ok(search_paged(session, top, batch, span)?.0)
}

/// Max UID SEARCH pages per sync pass. Dense mailboxes finish in one page;
/// the cap only bounds pathological UID gaps (mass deletions) to a handful
/// of cheap UID-only round-trips.
const SEARCH_PAGES: u32 = 8;

/// SEARCH UID space backwards from `top`, paging down until `want` UIDs are
/// known or UID 1 is reached.
///
/// UID ranges go sparse after mass deletions, so a single fixed window can
/// cover far fewer messages than asked for — the list would under-fill and
/// deletions below the window would linger locally as ghosts. Paging keeps
/// going while the take is short, so the newest-N window is genuinely the
/// newest N and the expunge diff covers everything asked about.
fn search_paged(
    session: &mut TlsSession,
    top: u32,
    want: usize,
    span: u32,
) -> Result<(HashSet<u32>, u32)> {
    let span = span.max(1);
    let mut found = HashSet::new();
    let mut hi = top;
    let mut lo = hi.saturating_sub(span.saturating_sub(1)).max(1);
    for _ in 0..SEARCH_PAGES {
        found.extend(session.uid_search(format!("UID {lo}:{hi}"))?);
        if found.len() >= want || lo <= 1 {
            break;
        }
        hi = lo - 1;
        lo = hi.saturating_sub(span.saturating_sub(1)).max(1);
    }
    if lo > 1 && found.len() < want {
        log::debug!(
            "imap: UID space still sparse after {SEARCH_PAGES} SEARCH pages, stopping at {lo}"
        );
    }
    Ok((found, lo))
}

fn flag_state(flags: &[Flag]) -> (bool, bool, bool) {
    let mut read = false;
    let mut starred = false;
    let mut draft = false;
    for f in flags {
        match f {
            Flag::Seen => read = true,
            Flag::Flagged => starred = true,
            Flag::Draft => draft = true,
            _ => {}
        }
    }
    (read, starred, draft)
}

/// Parse a raw RFC822 message into a storable [`NewMessage`] plus its
/// attachments. `with_bytes=true` copies part bytes (on-demand download);
/// `false` stores names/sizes only (background sync never pays for bytes).
fn parse_to_new(
    account_id: i64,
    folder_id: i64,
    uid: u32,
    flags: &[Flag],
    raw: &[u8],
    with_bytes: bool,
) -> Result<(NewMessage, Vec<NewAttachment>)> {
    let parsed = mail_parser::MessageParser::default()
        .parse(raw)
        .ok_or_else(|| StoreError::InvalidInput(format!("cannot parse message uid {uid}")))?;
    let (is_read, is_starred, is_draft) = flag_state(flags);
    // A draft you wrote is not "unread": never badge it, pill it, or dot
    // its row (see the flag-refresh loop, which applies the same rule).
    let is_read = is_read || is_draft;

    let body_text = parsed.body_text(0).map(|c| c.into_owned());
    let snippet = body_text.as_deref().map(|t| {
        let flat: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
        flat.chars().take(200).collect()
    });
    let date = parsed
        .date()
        .map(|d| d.to_timestamp())
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));

    // Thread root: last References id, else In-Reply-To.
    let thread_id = parsed
        .header("References")
        .and_then(|h| h.as_text())
        .and_then(|t| t.split_whitespace().last().map(str::to_string))
        .or_else(|| {
            parsed
                .header("In-Reply-To")
                .and_then(|h| h.as_text())
                .map(str::to_string)
        });

    let files = extract_attachments(&parsed, with_bytes);
    let has_attachments = parsed.attachment_count() > 0 || !files.is_empty();
    // Preserve exactly the RFC 5322 header block for the technical reader
    // view. It is bounded by the first blank line and never includes bodies.
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len());
    let raw_headers = String::from_utf8_lossy(&raw[..header_end]).to_string();

    Ok((
        NewMessage {
            account_id,
            folder_id,
            uid,
            message_id_header: parsed.message_id().map(str::to_string),
            thread_id,
            subject: parsed.subject().map(str::to_string),
            from_addr: parsed
                .from()
                .and_then(|a| a.first())
                .and_then(|a| a.address.as_ref().map(|s| s.to_string()))
                .or_else(|| {
                    parsed
                        .header("From")
                        .and_then(|h| h.as_text())
                        .map(str::to_string)
                }),
            to_addrs: addr_list(parsed.to()),
            cc_addrs: addr_list(parsed.cc()),
            bcc_addrs: addr_list(parsed.bcc()),
            reply_to: parsed
                .reply_to()
                .and_then(|a| a.first())
                .and_then(|a| a.address.as_ref().map(|s| s.to_string())),
            date,
            snippet,
            body_text,
            body_html: parsed.body_html(0).map(|c| c.into_owned()),
            raw_headers: (!raw_headers.is_empty()).then_some(raw_headers),
            is_read,
            is_starred,
            is_draft,
            has_attachments,
            keywords: Vec::new(),
            size: raw.len() as u64,
            downloaded_full: true,
        },
        files,
    ))
}

/// Pull attachment parts out of a parsed message.
///
/// Inline `cid:` images come along too (`is_inline=true`) — the reader keeps
/// showing them from the HTML, and the attachment bar lists only the real
/// files. Oversized parts are skipped (see [`MAX_ATTACHMENT_BYTES`]).
/// With `with_bytes=false` only names/sizes are kept (`data=None`): this is
/// what background sync stores, so no attachment bytes cross the network
/// until the user explicitly asks for a file.
fn extract_attachments(parsed: &mail_parser::Message<'_>, with_bytes: bool) -> Vec<NewAttachment> {
    use mail_parser::{MimeHeaders, PartType};
    let mut out = Vec::new();
    for part in parsed.attachments().take(MAX_ATTACHMENTS_PER_MESSAGE) {
        let len = match &part.body {
            PartType::Binary(b) | PartType::InlineBinary(b) => b.len(),
            PartType::Text(t) | PartType::Html(t) => t.len(),
            PartType::Message(nested) => nested.raw_message.len(),
            PartType::Multipart(_) => 0,
        };
        if len > MAX_ATTACHMENT_BYTES {
            log::warn!(
                "imap: skipping oversized attachment ({} bytes, name {:?})",
                len,
                part.attachment_name()
            );
            continue;
        }
        // Empty parts carry nothing worth storing (e.g. a zero-length
        // alternative body the parser classified as an attachment).
        if len == 0 {
            continue;
        }
        // Bytes are copied only on explicit request; background sync keeps
        // names/sizes so the download decision stays with the user.
        let data: Option<Vec<u8>> = if with_bytes {
            match &part.body {
                PartType::Binary(b) | PartType::InlineBinary(b) => Some(b.to_vec()),
                PartType::Text(t) | PartType::Html(t) => Some(t.as_bytes().to_vec()),
                PartType::Message(nested) => Some(nested.raw_message.to_vec()),
                PartType::Multipart(_) => None,
            }
        } else {
            None
        };
        if with_bytes && data.as_ref().is_none_or(|b| b.is_empty()) {
            continue;
        };
        let mime_type = part.content_type().map(|ct| match &ct.c_subtype {
            Some(sub) => format!(
                "{}/{}",
                ct.c_type.to_ascii_lowercase(),
                sub.to_ascii_lowercase()
            ),
            None => ct.c_type.to_ascii_lowercase(),
        });
        let is_inline = matches!(part.body, PartType::InlineBinary(_));
        out.push(NewAttachment {
            filename: part.attachment_name().map(str::to_string),
            mime_type,
            content_id: part.content_id().map(str::to_string),
            size: len as u64,
            data,
            is_inline,
        });
    }
    out
}

fn store_attachments(db: &Db, message_id: i64, files: Vec<NewAttachment>) -> Result<()> {
    messages::replace_attachments(db, message_id, &files)
}

/// Store attachment metadata (names/sizes, no bytes) for a freshly synced
/// message — unless rows already exist, in which case an earlier on-demand
/// download's bytes must survive the resync untouched.
fn store_attachment_meta(db: &Db, message_id: i64, files: Vec<NewAttachment>) {
    if files.is_empty() {
        return;
    }
    match messages::list_attachments(db, message_id) {
        Ok(existing) if !existing.is_empty() => return,
        Err(e) => {
            log::warn!("imap: cannot list attachments for {message_id}: {e}");
            return;
        }
        _ => {}
    }
    for f in &files {
        let meta = NewAttachment {
            data: None,
            ..f.clone()
        };
        if let Err(e) = messages::add_attachment(db, message_id, &meta) {
            log::warn!("imap: cannot store attachment {:?}: {e}", f.filename);
        }
    }
}

fn addr_list(a: Option<&mail_parser::Address>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(a) = a {
        for addr in a.iter() {
            if let Some(email) = addr.address.as_ref() {
                out.push(email.to_string());
            }
        }
    }
    out
}

fn collect_contacts_from_headers(db: &Db, raw_headers: Option<&str>) {
    if !settings::get_bool(db, settings::COLLECT_SENT_CONTACTS).unwrap_or(true) {
        return;
    }
    let Some(headers) = raw_headers else {
        return;
    };
    if headers.trim().is_empty() {
        return;
    }
    if let Some(parsed) = mail_parser::MessageParser::default().parse(headers.as_bytes()) {
        let collect = |addr_list: Option<&mail_parser::Address>| {
            if let Some(addrs) = addr_list {
                for a in addrs.iter() {
                    if let Some(email) = a.address.as_deref() {
                        let name = a.name.as_deref();
                        if let Err(e) = contacts::seen(db, email, name) {
                            log::warn!("contacts: could not collect contact {email}: {e}");
                        }
                    }
                }
            }
        };
        collect(parsed.from());
        collect(parsed.to());
        collect(parsed.cc());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_download_preserves_metadata_ids() {
        let db = Db::open_in_memory().unwrap();
        let account_id = accounts::create(
            &db,
            &crate::models::NewAccount {
                name: "Test".to_string(),
                email_address: "alice@example.com".to_string(),
                from_name: String::new(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "alice@example.com".to_string(),
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "alice@example.com".to_string(),
                auth_vault_key: "test".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let folder_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        let message_id =
            messages::upsert(&db, &messages::sample_new(account_id, folder_id, 1)).unwrap();
        let files: Vec<_> = [b"first".to_vec(), b"other".to_vec()]
            .into_iter()
            .map(|data| NewAttachment {
                filename: Some("notes.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                content_id: None,
                size: data.len() as u64,
                data: Some(data),
                is_inline: false,
            })
            .collect();
        store_attachment_meta(&db, message_id, files.clone());
        let metadata = messages::list_attachments(&db, message_id).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..2 {
            store_attachments(&db, message_id, files.clone()).unwrap();
            assert_eq!(
                messages::list_attachments(&db, message_id).unwrap().len(),
                2
            );
            for (original, expected) in metadata.iter().zip(&files) {
                let downloaded = messages::get_attachment(&db, original.id).unwrap();
                assert_eq!(downloaded.data, expected.data);
                let path = dir.path().join(original.id.to_string());
                messages::save_attachment_to_path(&db, original.id, &path).unwrap();
                assert_eq!(
                    std::fs::read(path).unwrap(),
                    expected.data.as_ref().unwrap().as_slice()
                );
            }
        }
    }

    #[test]
    fn endpoint_defaults_to_implicit_tls_on_993() {
        let a = crate::models::Account {
            id: 1,
            name: "n".to_string(),
            email_address: "e".to_string(),
            from_name: String::new(),
            imap_host: "imap.x".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "u".to_string(),
            smtp_host: "s".to_string(),
            smtp_port: 465,
            smtp_security: "tls".to_string(),
            smtp_username: "u".to_string(),
            auth_vault_key: "k".to_string(),
            check_interval_secs: 300,
            created_at: "t".to_string(),
            updated_at: "t".to_string(),
        };
        let ep = endpoint_for(&a);
        assert_eq!(ep.addr, "imap.x:993");
        assert!(ep.implicit_tls);
        assert!(!ep.starttls);
    }

    #[test]
    fn endpoint_honours_starttls_even_on_993() {
        let mut a = crate::models::Account {
            id: 1,
            name: "n".to_string(),
            email_address: "e".to_string(),
            from_name: String::new(),
            imap_host: "imap.x".to_string(),
            imap_port: 143,
            imap_security: "starttls".to_string(),
            imap_username: "u".to_string(),
            smtp_host: "s".to_string(),
            smtp_port: 587,
            smtp_security: "starttls".to_string(),
            smtp_username: "u".to_string(),
            auth_vault_key: "k".to_string(),
            check_interval_secs: 300,
            created_at: "t".to_string(),
            updated_at: "t".to_string(),
        };
        let ep = endpoint_for(&a);
        assert!(!ep.implicit_tls);
        assert!(ep.starttls);
        a.imap_port = 993;
        let ep = endpoint_for(&a);
        assert!(!ep.implicit_tls);
        assert!(ep.starttls);
        a.imap_security = "plain".to_string();
        a.imap_port = 143;
        let ep = endpoint_for(&a);
        assert!(!ep.implicit_tls);
        assert!(!ep.starttls);
        assert!(ImapSync::new(&a).connect("pw").is_err());
    }

    #[test]
    fn fresh_session_is_not_healthy() {
        // Offline: never dials out — a never-connected session has nothing
        // to NOOP against.
        let a = crate::models::Account {
            id: 1,
            name: "n".to_string(),
            email_address: "e".to_string(),
            from_name: String::new(),
            imap_host: "imap.x".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "u".to_string(),
            smtp_host: "s".to_string(),
            smtp_port: 465,
            smtp_security: "tls".to_string(),
            smtp_username: "u".to_string(),
            auth_vault_key: "k".to_string(),
            check_interval_secs: 300,
            created_at: "t".to_string(),
            updated_at: "t".to_string(),
        };
        assert!(!ImapSync::new(&a).is_healthy());
    }

    #[test]
    fn folder_path_normalization() {
        assert_eq!(normalize_folder_path("  Work  ", "/").unwrap(), "Work");
        assert_eq!(
            normalize_folder_path("Work/Client", "/").unwrap(),
            "Work/Client"
        );
        // "/" maps onto dotted hierarchies (Tobit David).
        assert_eq!(
            normalize_folder_path("Work/Client", ".").unwrap(),
            "Work.Client"
        );
        assert!(normalize_folder_path("", "/").is_err());
        assert!(normalize_folder_path("   ", "/").is_err());
        assert!(normalize_folder_path("a//b", "/").is_err());
        assert!(normalize_folder_path("/Lead", "/").is_err());
        assert!(normalize_folder_path("100% x", "/").is_err());
        assert!(normalize_folder_path("a*b", "/").is_err());
    }

    #[test]
    fn role_heuristics_cover_german_and_english_names() {
        assert_eq!(role_from_name("INBOX"), FolderRole::Inbox);
        assert_eq!(role_from_name("INBOX.Gesendet"), FolderRole::Sent);
        assert_eq!(role_from_name("[Gmail]/Sent Mail"), FolderRole::Sent);
        assert_eq!(role_from_name("Entwürfe"), FolderRole::Drafts);
        assert_eq!(role_from_name("Papierkorb"), FolderRole::Trash);
        assert_eq!(role_from_name("Spam"), FolderRole::Junk);
        assert_eq!(role_from_name("Archiv"), FolderRole::Archive);
        assert_eq!(role_from_name("INBOX.Projekte.Kunde"), FolderRole::Custom);
        assert_eq!(role_from_name("Family"), FolderRole::Custom);
        assert_eq!(role_from_name("Cabin"), FolderRole::Custom);
        assert_eq!(role_from_name("Binders"), FolderRole::Custom);
        assert_eq!(role_from_name("INBOX.Bin"), FolderRole::Trash);
        assert_eq!(role_from_name("Drafts"), FolderRole::Drafts);
    }

    #[test]
    fn namespace_parser_survives_hostile_bytes() {
        // Fuzz the NAMESPACE parser with adversarial inputs: it must never
        // panic, only return partial/empty results. (A startup-sync abort was
        // traced to this code path against a quirky groupware server.)
        let mut state: u64 = 0x12345678abcdef;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let pieces: &[&[u8]] = &[
            b"* NAMESPACE ",
            b"((",
            b"))",
            b"(",
            b")",
            b"\"\"",
            b"\"/\"",
            b"\".\"",
            b"\"INBOX.\"",
            b"NIL",
            b"N",
            b"NI",
            b" ",
            b"\"",
            b"\\",
            b"\\\"",
            b"\xc3\xa4",
            b"\xff\xfe",
            b"\x80",
            b"A",
            b"*",
        ];
        for _ in 0..50_000 {
            let mut buf = Vec::new();
            let n = (next() % 8) as usize;
            for _ in 0..n {
                buf.extend_from_slice(pieces[(next() % pieces.len() as u64) as usize]);
            }
            let _ = parse_namespace_response(&buf);
        }
    }

    #[test]
    fn namespace_response_parses_three_groups() {
        let raw =
            b"* NAMESPACE ((\"\" \"/\")) ((\"Other Users/\" \"/\")) ((\"Shared/\" \"/\"))\r\n\
            a001 OK done\r\n";
        let ns = parse_namespace_response(raw);
        assert_eq!(ns.personal, vec![("".to_string(), Some("/".to_string()))]);
        assert_eq!(
            ns.other,
            vec![("Other Users/".to_string(), Some("/".to_string()))]
        );
        assert_eq!(
            ns.shared,
            vec![("Shared/".to_string(), Some("/".to_string()))]
        );
    }

    #[test]
    fn namespace_response_tolerates_nil_and_garbage() {
        let raw = b"* NAMESPACE ((\"INBOX.\" \".\")) NIL NIL\r\n";
        let ns = parse_namespace_response(raw);
        assert_eq!(
            ns.personal,
            vec![("INBOX.".to_string(), Some(".".to_string()))]
        );
        assert!(ns.other.is_empty() && ns.shared.is_empty());

        assert_eq!(
            parse_namespace_response(b"a001 BAD no\r\n"),
            Namespaces::default()
        );
        assert_eq!(parse_namespace_response(b""), Namespaces::default());
    }

    #[test]
    fn parse_captures_reply_to() {
        let raw = b"From: alice@example.com\r\n\
            Reply-To: replies@example.org\r\n\
            To: bob@example.com\r\n\
            Subject: reply here\r\n\
            Message-ID: <a4@example.com>\r\n\
            Date: Mon, 07 Sep 2026 10:00:00 +0000\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            body\r\n";
        let (msg, _) = parse_to_new(1, 1, 45, &[], raw, false).unwrap();
        assert_eq!(msg.reply_to.as_deref(), Some("replies@example.org"));
        // No Reply-To header: stays empty rather than echoing From.
        let raw2 = b"From: alice@example.com\r\n\
            To: bob@example.com\r\n\
            Subject: no reply-to\r\n\
            Message-ID: <a5@example.com>\r\n\
            Date: Mon, 07 Sep 2026 10:00:00 +0000\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            body\r\n";
        let (msg2, _) = parse_to_new(1, 1, 46, &[], raw2, false).unwrap();
        assert!(msg2.reply_to.is_none());
    }

    #[test]
    fn parse_extracts_attachment_bytes() {
        // multipart/mixed with a text body + one base64 file.
        let raw = b"From: alice@example.com\r\n\
            To: bob@example.com\r\n\
            Subject: files\r\n\
            Message-ID: <a1@example.com>\r\n\
            Date: Mon, 07 Sep 2026 10:00:00 +0000\r\n\
            Content-Type: multipart/mixed; boundary=\"B\"\r\n\
            \r\n\
            --B\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            see attached\r\n\
            --B\r\n\
            Content-Type: application/pdf; name=\"doc.pdf\"\r\n\
            Content-Disposition: attachment; filename=\"doc.pdf\"\r\n\
            Content-Transfer-Encoding: base64\r\n\
            \r\n\
            aGVsbG8td29ybGQ=\r\n\
            --B--\r\n";
        let (msg, files) = parse_to_new(1, 1, 42, &[], raw, true).unwrap();
        assert!(msg.has_attachments);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename.as_deref(), Some("doc.pdf"));
        assert_eq!(files[0].mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(files[0].size, 11);
        assert_eq!(files[0].data.as_deref(), Some(b"hello-world".as_slice()));
        assert!(!files[0].is_inline);
    }

    #[test]
    fn parse_skips_empty_parts_but_keeps_flag() {
        // A zero-length attachment part carries nothing to store, but the
        // message still had an attachment on the wire.
        let raw = b"From: alice@example.com\r\n\
            To: bob@example.com\r\n\
            Subject: empty\r\n\
            Message-ID: <a2@example.com>\r\n\
            Date: Mon, 07 Sep 2026 10:00:00 +0000\r\n\
            Content-Type: multipart/mixed; boundary=\"B\"\r\n\
            \r\n\
            --B\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            body\r\n\
            --B\r\n\
            Content-Type: application/octet-stream; name=\"empty.bin\"\r\n\
            Content-Disposition: attachment; filename=\"empty.bin\"\r\n\
            \r\n\
            --B--\r\n";
        let (msg, files) = parse_to_new(1, 1, 43, &[], raw, true).unwrap();
        assert!(msg.has_attachments);
        assert!(files.is_empty());
    }

    #[test]
    fn parse_meta_mode_keeps_names_without_bytes() {
        // Background sync: the same wire bytes yield names/sizes but no
        // payload, so nothing downloads until the user asks for a file.
        let raw = b"From: alice@example.com\r\n\
            To: bob@example.com\r\n\
            Subject: files\r\n\
            Message-ID: <a3@example.com>\r\n\
            Date: Mon, 07 Sep 2026 10:00:00 +0000\r\n\
            Content-Type: multipart/mixed; boundary=\"B\"\r\n\
            \r\n\
            --B\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            see attached\r\n\
            --B\r\n\
            Content-Type: application/pdf; name=\"doc.pdf\"\r\n\
            Content-Disposition: attachment; filename=\"doc.pdf\"\r\n\
            Content-Transfer-Encoding: base64\r\n\
            \r\n\
            aGVsbG8td29ybGQ=\r\n\
            --B--\r\n";
        let (msg, files) = parse_to_new(1, 1, 44, &[], raw, false).unwrap();
        assert!(msg.has_attachments);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].filename.as_deref(), Some("doc.pdf"));
        assert_eq!(files[0].size, 11);
        assert!(files[0].data.is_none());
    }
}
