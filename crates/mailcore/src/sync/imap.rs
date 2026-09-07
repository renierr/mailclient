//! IMAP sync (Milestone 1): connect, LIST folders with role mapping,
//! SELECT + UID FETCH into SQLite, flag push, expunge handling.
//!
//! Transport: implicit TLS (port 993). Anything else is rejected unless the
//! account explicitly opts into STARTTLS/plain (see AGENT.md security rules).

use std::collections::HashSet;
use std::net::TcpStream;

use imap::types::{Flag, NameAttribute};
use native_tls::{TlsConnector, TlsStream};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Folder, FolderRole, Message, NewMessage};
use crate::store::{accounts, folders, messages};
use crate::sync::traits::{SyncProvider, SyncReport};

type TlsSession = imap::Session<TlsStream<TcpStream>>;

/// How many UIDs per FETCH round-trip.
const FETCH_CHUNK: usize = 100;

/// Resolved IMAP endpoint for one account.
#[derive(Debug, Clone)]
pub struct ImapEndpoint {
    /// `host:port`.
    pub addr: String,
    /// `true` for implicit TLS (993), `false` for STARTTLS/plain (143 + opt-in).
    pub implicit_tls: bool,
}

/// Derive the endpoint from account settings.
#[must_use]
pub fn endpoint_for(account: &crate::models::Account) -> ImapEndpoint {
    ImapEndpoint {
        addr: format!("{}:{}", account.imap_host, account.imap_port),
        implicit_tls: account.imap_port == 993
            || account.imap_security.eq_ignore_ascii_case("tls"),
    }
}

/// Whether a LISTED mailbox can hold messages (i.e. not `\Noselect`).
#[must_use]
pub fn is_selectable(attributes: &[NameAttribute]) -> bool {
    !attr_text(attributes).contains("noselect")
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
#[must_use]
pub fn role_from_name(name: &str) -> FolderRole {
    let lower = name.to_ascii_lowercase();
    if lower == "inbox" {
        return FolderRole::Inbox;
    }
    let last = lower.rsplit(['/', '.']).next().unwrap_or(&lower);
    let has = |words: &[&str]| words.iter().any(|w| last.contains(w));
    if has(&["sent", "gesendet", "sent mail", "sent items"]) {
        FolderRole::Sent
    } else if has(&["draft", "entwurf", "entwürf"]) {
        FolderRole::Drafts
    } else if has(&["trash", "deleted", "papierkorb", "gelöscht", "bin"]) {
        FolderRole::Trash
    } else if has(&["junk", "spam"]) {
        FolderRole::Junk
    } else if has(&["archive", "archiv"]) {
        FolderRole::Archive
    } else {
        FolderRole::Custom
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

/// IMAP sync session. Construct with [`ImapSync::new`], then [`ImapSync::connect`].
pub struct ImapSync {
    host: String,
    port: u16,
    username: String,
    implicit_tls: bool,
    session: Option<TlsSession>,
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
            session: None,
        }
    }

    /// Dial + LOGIN. Password comes from the OS keyring (or test env).
    pub fn connect(&mut self, password: &str) -> Result<()> {
        if self.session.is_some() {
            return Ok(());
        }
        if !self.implicit_tls {
            return Err(StoreError::InvalidInput(format!(
                "refusing non-TLS IMAP for {} (explicit opt-in required)",
                self.host
            )));
        }
        log::info!("imap: connecting to {}:{}", self.host, self.port);
        let tls = TlsConnector::builder().build()?;
        let client = imap::connect((self.host.as_str(), self.port), &self.host, &tls)?;
        let session = client
            .login(self.username.as_str(), password)
            .map_err(|(e, _)| StoreError::Imap(e))?;
        log::info!("imap: logged in as {}", self.username);
        self.session = Some(session);
        Ok(())
    }

    /// LOGOUT (best effort).
    pub fn disconnect(&mut self) {
        if let Some(mut s) = self.session.take() {
            let _ = s.logout();
        }
    }

    /// APPEND raw MIME bytes to a folder (used for Sent copies), marked `\Seen`.
    pub fn append_to_folder(&mut self, folder_path: &str, raw: &[u8]) -> Result<()> {
        self.session()?
            .append_with_flags(folder_path, raw, &[Flag::Seen])?;
        Ok(())
    }

    fn session(&mut self) -> Result<&mut TlsSession> {
        self.session.as_mut().ok_or_else(|| {
            StoreError::InvalidInput("not connected: call connect() first".to_string())
        })
    }
}

impl SyncProvider for ImapSync {
    fn name(&self) -> &'static str {
        "imap"
    }

    fn sync_folders(&mut self, db: &Db, account_id: i64) -> Result<Vec<Folder>> {
        let names = self.session()?.list(Some(""), Some("*"))?;
        let mut out = Vec::new();
        let mut count = 0u64;
        for n in names.iter() {
            if !is_selectable(n.attributes()) {
                log::debug!("imap: skipping non-selectable {}", n.name());
                continue;
            }
            let role = map_folder_role(n.attributes(), n.name());
            let delimiter = n.delimiter().unwrap_or("/");
            let id = folders::upsert(db, account_id, n.name(), delimiter, role)?;
            log::info!("imap: folder {} -> {}", n.name(), role.as_str());
            out.push(folders::get(db, id)?);
            count += 1;
        }
        log::info!("imap: {count} folders");
        let _ = count;
        Ok(out)
    }

    fn sync_folder(&mut self, db: &Db, folder_id: i64) -> Result<SyncReport> {
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
                log::warn!(
                    "imap: UIDVALIDITY changed for {} — resyncing",
                    folder.path
                );
                messages::delete_by_folder(db, folder_id)?;
            }
        }

        let server_uids: HashSet<u32> = session.uid_search("ALL")?;
        let local_uids: HashSet<u32> =
            messages::list_uids(db, folder_id)?.into_iter().collect();

        // 1. Flag refresh for messages we already have.
        let existing: Vec<u32> =
            server_uids.intersection(&local_uids).copied().collect();
        for chunk in existing.chunks(FETCH_CHUNK) {
            let set = chunk.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            for msg in session.uid_fetch(set, "(UID FLAGS)")?.iter() {
                if let Some(uid) = msg.uid {
                    let (read, starred, draft) = flag_state(msg.flags());
                    messages::set_flags_by_uid(
                        db,
                        account.id,
                        folder_id,
                        uid,
                        read,
                        starred,
                        draft,
                    )?;
                }
            }
        }

        // 2. Full fetch of new messages.
        let mut fetched = 0u64;
        let missing: Vec<u32> = server_uids.difference(&local_uids).copied().collect();
        for chunk in missing.chunks(FETCH_CHUNK) {
            let set = chunk.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
            for msg in session.uid_fetch(set, "(UID FLAGS RFC822)")?.iter() {
                let uid = msg.uid.unwrap_or(0);
                if uid == 0 {
                    continue;
                }
                let raw = msg.body().unwrap_or_default();
                let parsed = parse_to_new(account.id, folder_id, uid, msg.flags(), raw)?;
                messages::upsert(db, &parsed)?;
                fetched += 1;
            }
        }

        // 3. Expunge locally what the server no longer has.
        let mut expunged = 0u64;
        for uid in local_uids.difference(&server_uids) {
            messages::delete_by_uid(db, folder_id, *uid)?;
            expunged += 1;
        }

        let validity = mb
            .uid_validity
            .unwrap_or(folder.uid_validity.unwrap_or(0));
        let uid_next = mb.uid_next.unwrap_or(folder.uid_next.unwrap_or(0));
        folders::set_sync_state(db, folder_id, validity, uid_next)?;

        Ok(SyncReport { fetched, expunged, folders: 0 })
    }

    fn push_flags(&mut self, db: &Db, message: &Message) -> Result<()> {
        let folder = folders::get(db, message.folder_id)?;
        let session = self.session()?;
        session.select(&folder.path)?;
        let uid = message.uid.to_string();
        let seen = if message.is_read { "+FLAGS" } else { "-FLAGS" };
        let flagged = if message.is_starred { "+FLAGS" } else { "-FLAGS" };
        session.uid_store(&uid, format!("{seen} (\\Seen)"))?;
        session.uid_store(&uid, format!("{flagged} (\\Flagged)"))?;
        Ok(())
    }
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

/// Parse a raw RFC822 message into a storable [`NewMessage`].
fn parse_to_new(
    account_id: i64,
    folder_id: i64,
    uid: u32,
    flags: &[Flag],
    raw: &[u8],
) -> Result<NewMessage> {
    let parsed = mail_parser::MessageParser::default()
        .parse(raw)
        .ok_or_else(|| {
            StoreError::InvalidInput(format!("cannot parse message uid {uid}"))
        })?;
    let (is_read, is_starred, is_draft) = flag_state(flags);

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

    Ok(NewMessage {
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
        reply_to: None,
        date,
        snippet,
        body_text,
        body_html: parsed.body_html(0).map(|c| c.into_owned()),
        is_read,
        is_starred,
        is_draft,
        has_attachments: parsed.attachment_count() > 0,
        keywords: Vec::new(),
        size: raw.len() as u64,
        downloaded_full: true,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_defaults_to_implicit_tls_on_993() {
        let a = crate::models::Account {
            id: 1,
            name: "n".to_string(),
            email_address: "e".to_string(),
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
    }
}
