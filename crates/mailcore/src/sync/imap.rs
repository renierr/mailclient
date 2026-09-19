//! IMAP sync (Milestone 1 + 5a CONDSTORE/QRESYNC): connect, LIST folders with role mapping,
//! SELECT + UID FETCH into SQLite, flag push, CONDSTORE/QRESYNC delta sync, and expunge handling.
//!
//! Powered by `imap-next` (sans-I/O protocol state machine over Tokio).
//! Transport: implicit TLS (port 993) or STARTTLS (port 143). Plaintext is
//! refused unless the account explicitly opts in (see AGENT.md security rules).

use std::collections::HashSet;
use std::sync::Arc;

use core::num::{NonZeroU32, NonZeroU64};

use imap_next::{
    client::{Client, Event, Options},
    stream::Stream,
};
use imap_types::{
    command::{Command, CommandBody, FetchModifier, SelectParameter},
    core::{AString, Literal, Tag, Vec1},
    extensions::{binary::LiteralOrLiteral8, enable::CapabilityEnable},
    fetch::{MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName},
    flag::{Flag, FlagFetch, FlagNameAttribute, StoreResponse, StoreType},
    mailbox::Mailbox,
    response::{Code, Data, Status, StatusBody, StatusKind},
    search::SearchKey,
    sequence::{SeqOrUid, Sequence, SequenceSet},
    IntoStatic,
};
use rustls_pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::{rustls, TlsConnector};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Folder, FolderRole, Message, NewAttachment, NewMessage};
use crate::store::{accounts, contacts, folders, messages, settings};
use crate::sync::traits::{SyncProvider, SyncReport};

macro_rules! vec1 {
    ($($x:expr),+ $(,)?) => {
        ::imap_types::core::Vec1::try_from(vec![$($x),+]).expect("vec1 cannot be empty")
    };
}

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
    pub host: String,
    pub port: u16,
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
        host: account.imap_host.clone(),
        port: account.imap_port,
        implicit_tls,
        starttls,
    }
}

/// Format name attributes into lowercase string for heuristic search.
pub fn attr_text(attributes: &[FlagNameAttribute<'_>]) -> String {
    attributes
        .iter()
        .map(|a| format!("{a:?}"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Whether a LISTED mailbox can hold messages (i.e. not `\Noselect` and not
/// a `\NonExistent` hierarchy placeholder).
#[must_use]
pub fn is_selectable(attributes: &[FlagNameAttribute<'_>]) -> bool {
    !attributes.iter().any(|a| {
        let s = a.to_string();
        s.eq_ignore_ascii_case("\\noselect") || s.eq_ignore_ascii_case("\\nonexistent")
    })
}

/// Map a LISTED mailbox to a [`FolderRole`].
///
/// Prefers RFC 6154 SPECIAL-USE attributes, falls back to multilingual name
/// heuristics, keeps everything else as `Custom` (user IMAP folders included).
#[must_use]
pub fn map_folder_role(attributes: &[FlagNameAttribute<'_>], name: &str) -> FolderRole {
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

/// Fallback role guessing based on the folder's leaf name.
#[must_use]
pub fn role_from_name(name: &str) -> FolderRole {
    let leaf = name
        .rsplit(['/', '.', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase();

    match leaf.as_str() {
        "inbox" => FolderRole::Inbox,
        "sent" | "sent items" | "sent messages" | "gesendet" | "gesendete elemente"
        | "gesendete objekte" => FolderRole::Sent,
        "drafts" | "draft" | "entwürfe" | "entwuerfe" => FolderRole::Drafts,
        "trash"
        | "deleted"
        | "deleted items"
        | "deleted messages"
        | "papierkorb"
        | "gelöschte elemente"
        | "geloeschte elemente"
        | "bin" => FolderRole::Trash,
        "junk" | "junk mail" | "junk email" | "spam" | "bulk mail" | "unerwünscht" => {
            FolderRole::Junk
        }
        "archive" | "archiv" => FolderRole::Archive,
        _ => FolderRole::Custom,
    }
}

/// Helper to build a TLS connector trusting system certificates with WebPKI roots fallback.
fn build_tls_connector() -> Result<TlsConnector> {
    let mut root_store = rustls::RootCertStore::empty();
    let native_certs = rustls_native_certs::load_native_certs();
    for cert in native_certs.certs {
        let _ = root_store.add(cert);
    }
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Result of executing an IMAP command.
#[derive(Debug)]
struct CommandResult {
    data: Vec<Data<'static>>,
    untagged_statuses: Vec<StatusBody<'static>>,
    status: Status<'static>,
}

/// Result of selecting a mailbox.
#[derive(Debug, Default, Clone)]
pub struct SelectResult {
    pub exists: u32,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    pub vanished: Vec<u32>,
}

/// Helper to extract all UIDs from a sequence set.
fn sequence_set_to_uids(set: &SequenceSet) -> Vec<u32> {
    let mut uids = Vec::new();
    for seq in set.0.as_ref() {
        match seq {
            Sequence::Single(SeqOrUid::Value(v)) => uids.push(v.get()),
            Sequence::Range(SeqOrUid::Value(a), SeqOrUid::Value(b)) => {
                let start = a.get().min(b.get());
                let end = a.get().max(b.get());
                for u in start..=end {
                    uids.push(u);
                }
            }
            _ => {}
        }
    }
    uids
}

/// Discovered folder from LIST/LSUB.
#[derive(Debug, Clone)]
pub struct DiscoveredFolder {
    pub name: String,
    pub delimiter: String,
    pub attributes: Vec<FlagNameAttribute<'static>>,
}

/// Active IMAP session using `imap-next`.
pub struct ImapSession {
    stream: Stream,
    client: Client,
    tag_counter: u64,
    capabilities: Vec<String>,
    condstore_enabled: bool,
    qresync_enabled: bool,
}

impl ImapSession {
    fn next_tag(&mut self) -> Tag<'static> {
        self.tag_counter += 1;
        Tag::try_from(format!("A{:04}", self.tag_counter)).expect("valid tag")
    }

    /// Read the initial server greeting.
    async fn read_greeting(stream: &mut Stream, client: &mut Client) -> Result<()> {
        loop {
            match stream
                .next(&mut *client)
                .await
                .map_err(|e| StoreError::Network(format!("greeting error: {e}")))?
            {
                Event::GreetingReceived { .. } => return Ok(()),
                event => {
                    log::debug!("imap: unexpected greeting event: {event:?}");
                }
            }
        }
    }

    /// Execute a command and wait for its completion.
    async fn execute(&mut self, body: CommandBody<'static>) -> Result<CommandResult> {
        let tag = self.next_tag();
        let cmd = Command::new(tag.clone(), body)
            .map_err(|e| StoreError::Imap(format!("invalid command: {e}")))?;
        let handle = self.client.enqueue_command(cmd);

        let mut collected_data = Vec::new();
        let mut untagged_statuses = Vec::new();

        loop {
            let event = self
                .stream
                .next(&mut self.client)
                .await
                .map_err(|e| StoreError::Network(format!("stream error: {e}")))?;

            match event {
                Event::CommandSent { handle: h, .. } if h == handle => {
                    // command sent
                }
                Event::CommandRejected {
                    handle: h, status, ..
                } if h == handle => {
                    return Err(StoreError::Network(format!("command rejected: {status:?}")));
                }
                Event::DataReceived { data } => {
                    collected_data.push(data.into_static());
                }
                Event::StatusReceived { status } => match status {
                    Status::Tagged(tagged) if tagged.tag == tag => match tagged.body.kind {
                        StatusKind::Ok => {
                            return Ok(CommandResult {
                                data: collected_data,
                                untagged_statuses,
                                status: Status::Tagged(tagged).into_static(),
                            });
                        }
                        StatusKind::No | StatusKind::Bad => {
                            return Err(StoreError::Network(format!(
                                "server returned {:?}: {}",
                                tagged.body.kind, tagged.body.text
                            )));
                        }
                    },
                    Status::Untagged(untagged) => {
                        untagged_statuses.push(untagged.into_static());
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }

    /// Authenticate with LOGIN.
    pub async fn login(&mut self, user: &str, pass: &str) -> Result<()> {
        let body = CommandBody::login(user.to_string(), pass.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("login args invalid: {e}")))?;
        self.execute(body).await?;
        Ok(())
    }

    /// Query CAPABILITY.
    pub async fn capability(&mut self) -> Result<Vec<String>> {
        let res = self.execute(CommandBody::Capability).await?;
        let mut caps = Vec::new();
        for d in res.data {
            if let Data::Capability(c) = d {
                for cap in c.as_ref() {
                    caps.push(cap.to_string());
                }
            }
        }
        let check_code = |code: &Option<Code<'_>>, caps: &mut Vec<String>| {
            if let Some(Code::Capability(c)) = code {
                for cap in c.as_ref() {
                    caps.push(cap.to_string());
                }
            }
        };
        for s in &res.untagged_statuses {
            check_code(&s.code, &mut caps);
        }
        if let Status::Tagged(t) = &res.status {
            check_code(&t.body.code, &mut caps);
        }
        caps.sort();
        caps.dedup();
        self.capabilities = caps.clone();
        Ok(caps)
    }

    /// Check if a capability is present (case-insensitive).
    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c.eq_ignore_ascii_case(cap))
    }

    /// Try to enable CONDSTORE and QRESYNC if advertised.
    pub async fn enable_extensions(&mut self) -> Result<()> {
        let has_enable = self.has_capability("enable");
        let has_condstore = self.has_capability("condstore");
        let has_qresync = self.has_capability("qresync");

        if !has_condstore && !has_qresync {
            return Ok(());
        }

        if has_enable {
            let mut enable_caps = Vec::new();
            if has_condstore {
                if let Ok(cap) = CapabilityEnable::try_from("CONDSTORE") {
                    enable_caps.push(cap);
                }
            }
            if has_qresync {
                if let Ok(cap) = CapabilityEnable::try_from("QRESYNC") {
                    enable_caps.push(cap);
                }
            }

            if let Ok(caps) = Vec1::try_from(enable_caps) {
                let body = CommandBody::enable(caps).unwrap();
                if let Ok(res) = self.execute(body).await {
                    for d in res.data {
                        if let Data::Enabled { capabilities } = d {
                            for cap in &capabilities {
                                let s = cap.to_string().to_ascii_lowercase();
                                if s == "condstore" {
                                    self.condstore_enabled = true;
                                }
                                if s == "qresync" {
                                    self.qresync_enabled = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        // If CONDSTORE was advertised without ENABLE (RFC 4551), it is activated via SELECT.
        if has_condstore && !self.condstore_enabled {
            self.condstore_enabled = true;
        }

        log::info!(
            "imap: extensions enabled: condstore={}, qresync={}",
            self.condstore_enabled,
            self.qresync_enabled
        );
        Ok(())
    }

    /// SELECT a folder, with optional QRESYNC parameters and automatic fallback.
    pub async fn select(
        &mut self,
        path: &str,
        qresync: Option<(u32, u64)>,
    ) -> Result<SelectResult> {
        let mailbox = Mailbox::try_from(path.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {path}: {e}")))?;

        let (body, has_extension) = if self.qresync_enabled && qresync.is_some_and(|(_, m)| m > 0) {
            let (validity, modseq) = qresync.unwrap();
            let nz_val = NonZeroU32::new(validity);
            let nz_mod = NonZeroU64::new(modseq);
            if let (Some(v), Some(m)) = (nz_val, nz_mod) {
                let param = SelectParameter::QResync {
                    uid_validity: v,
                    mod_sequence_value: m,
                    known_uids: None,
                    seq_match_data: None,
                };
                (
                    CommandBody::Select {
                        mailbox: mailbox.clone(),
                        parameters: vec![param],
                    },
                    true,
                )
            } else {
                (
                    CommandBody::Select {
                        mailbox: mailbox.clone(),
                        parameters: vec![SelectParameter::CondStore],
                    },
                    true,
                )
            }
        } else if self.condstore_enabled {
            (
                CommandBody::Select {
                    mailbox: mailbox.clone(),
                    parameters: vec![SelectParameter::CondStore],
                },
                true,
            )
        } else {
            (
                CommandBody::select(mailbox.clone()).map_err(|e| {
                    StoreError::InvalidInput(format!("invalid mailbox {path}: {e}"))
                })?,
                false,
            )
        };

        let res = match self.execute(body).await {
            Ok(r) => r,
            Err(e) if has_extension => {
                log::warn!(
                    "imap: SELECT with extension failed ({e}), falling back to standard SELECT"
                );
                self.condstore_enabled = false;
                self.qresync_enabled = false;
                let fallback = CommandBody::select(mailbox).map_err(|e| {
                    StoreError::InvalidInput(format!("invalid mailbox {path}: {e}"))
                })?;
                self.execute(fallback).await?
            }
            Err(e) => return Err(e),
        };

        let mut out = SelectResult::default();

        for d in res.data {
            match d {
                Data::Exists(n) => out.exists = n,
                Data::Vanished {
                    earlier: _,
                    known_uids,
                } => {
                    out.vanished.extend(sequence_set_to_uids(&known_uids));
                }
                _ => {}
            }
        }

        let mut check_code = |code: &Option<Code<'_>>| {
            if let Some(c) = code {
                match c {
                    Code::UidValidity(v) => out.uid_validity = Some(v.get()),
                    Code::UidNext(n) => out.uid_next = Some(n.get()),
                    Code::HighestModSeq(m) => out.highest_modseq = Some(m.get()),
                    _ => {}
                }
            }
        };

        for s in &res.untagged_statuses {
            check_code(&s.code);
        }
        if let Status::Tagged(t) = &res.status {
            check_code(&t.body.code);
        }

        Ok(out)
    }

    /// Fetch flags with optional CONDSTORE `CHANGEDSINCE`.
    pub async fn uid_fetch_flags_changesince(
        &mut self,
        uids: &[u32],
        modseq: u64,
    ) -> Result<Vec<(u32, Vec<Flag<'static>>, Option<u64>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set_str = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let sequence_set = SequenceSet::try_from(set_str.as_str())
            .map_err(|e| StoreError::InvalidInput(format!("invalid sequence set: {e}")))?;

        let (macro_or_item_names, modifiers) = if self.condstore_enabled && modseq > 0 {
            (
                MacroOrMessageDataItemNames::from(vec![
                    MessageDataItemName::Uid,
                    MessageDataItemName::Flags,
                    MessageDataItemName::ModSeq,
                ]),
                vec![FetchModifier::ChangedSince(
                    NonZeroU64::new(modseq).unwrap(),
                )],
            )
        } else {
            (
                MacroOrMessageDataItemNames::from(vec![
                    MessageDataItemName::Uid,
                    MessageDataItemName::Flags,
                ]),
                Vec::new(),
            )
        };

        let has_modifiers = !modifiers.is_empty();
        let body = CommandBody::Fetch {
            sequence_set: sequence_set.clone(),
            macro_or_item_names,
            uid: true,
            modifiers,
        };

        let res = match self.execute(body).await {
            Ok(r) => r,
            Err(e) if has_modifiers => {
                log::warn!(
                    "imap: UID FETCH CHANGEDSINCE failed ({e}), falling back to standard UID FETCH"
                );
                self.condstore_enabled = false;
                let fallback = CommandBody::Fetch {
                    sequence_set,
                    macro_or_item_names: MacroOrMessageDataItemNames::from(vec![
                        MessageDataItemName::Uid,
                        MessageDataItemName::Flags,
                    ]),
                    uid: true,
                    modifiers: Vec::new(),
                };
                self.execute(fallback).await?
            }
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();

        for d in res.data {
            if let Data::Fetch { items, .. } = d {
                let mut uid = 0u32;
                let mut flags = Vec::new();
                let mut msg_modseq = None;

                for item in items.as_ref() {
                    match item {
                        MessageDataItem::Uid(u) => uid = u.get(),
                        MessageDataItem::Flags(f) => {
                            for flag_fetch in f {
                                if let FlagFetch::Flag(flag) = flag_fetch {
                                    flags.push(flag.clone().into_static());
                                }
                            }
                        }
                        MessageDataItem::ModSeq(m) => msg_modseq = Some(m.get()),
                        _ => {}
                    }
                }
                if uid > 0 {
                    out.push((uid, flags, msg_modseq));
                }
            }
        }

        Ok(out)
    }

    /// Fetch full messages (UID, FLAGS, and raw RFC822 bodies).
    pub async fn uid_fetch_messages(
        &mut self,
        uids: &[u32],
    ) -> Result<Vec<(u32, Vec<Flag<'static>>, Vec<u8>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let set_str = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let sequence_set = SequenceSet::try_from(set_str.as_str())
            .map_err(|e| StoreError::InvalidInput(format!("invalid sequence set: {e}")))?;

        let body = CommandBody::Fetch {
            sequence_set,
            macro_or_item_names: MacroOrMessageDataItemNames::from(vec![
                MessageDataItemName::Uid,
                MessageDataItemName::Flags,
                MessageDataItemName::BodyExt {
                    section: None,
                    partial: None,
                    peek: true,
                },
            ]),
            uid: true,
            modifiers: Vec::new(),
        };

        let res = self.execute(body).await?;
        let mut out = Vec::new();

        for d in res.data {
            if let Data::Fetch { items, .. } = d {
                let mut uid = 0u32;
                let mut flags = Vec::new();
                let mut raw_body = Vec::new();

                for item in items.as_ref() {
                    match item {
                        MessageDataItem::Uid(u) => uid = u.get(),
                        MessageDataItem::Flags(f) => {
                            for flag_fetch in f {
                                if let FlagFetch::Flag(flag) = flag_fetch {
                                    flags.push(flag.clone().into_static());
                                }
                            }
                        }
                        MessageDataItem::BodyExt { data, .. } => {
                            if let Some(bytes) = data.0.as_ref().map(|s| s.as_ref()) {
                                raw_body = bytes.to_vec();
                            }
                        }
                        MessageDataItem::Rfc822(data) => {
                            if let Some(bytes) = data.0.as_ref().map(|s| s.as_ref()) {
                                raw_body = bytes.to_vec();
                            }
                        }
                        _ => {}
                    }
                }
                if uid > 0 {
                    out.push((uid, flags, raw_body));
                }
            }
        }

        Ok(out)
    }

    /// STORE flags (+FLAGS / -FLAGS).
    pub async fn uid_store_flags(
        &mut self,
        uids: &[u32],
        op: StoreType,
        flags: Vec<Flag<'static>>,
    ) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let set_str = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let sequence_set = SequenceSet::try_from(set_str.as_str())
            .map_err(|e| StoreError::InvalidInput(format!("invalid sequence set: {e}")))?;

        let body = CommandBody::store(sequence_set, op, StoreResponse::Silent, flags, true)
            .map_err(|e| StoreError::InvalidInput(format!("store args: {e}")))?;

        self.execute(body).await?;
        Ok(())
    }

    /// UID SEARCH with criteria.
    pub async fn uid_search(&mut self, criteria: Vec1<SearchKey<'static>>) -> Result<Vec<u32>> {
        let body = CommandBody::search(None, criteria, true);
        let res = self.execute(body).await?;
        let mut uids = Vec::new();
        for d in res.data {
            if let Data::Search(found, _) = d {
                uids.extend(found.iter().map(|n| n.get()));
            }
        }
        Ok(uids)
    }

    /// UID COPY.
    pub async fn uid_copy(&mut self, uids: &[u32], dest: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let set_str = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let sequence_set = SequenceSet::try_from(set_str.as_str())
            .map_err(|e| StoreError::InvalidInput(format!("invalid sequence set: {e}")))?;
        let mailbox = Mailbox::try_from(dest.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {dest}: {e}")))?;
        let body = CommandBody::copy(sequence_set, mailbox, true)
            .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
        self.execute(body).await?;
        Ok(())
    }

    /// UID MOVE (or fallback copy + deleted + expunge).
    pub async fn uid_move(&mut self, uids: &[u32], dest: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let has_move = self
            .capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case("move"));

        let set_str = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let sequence_set = SequenceSet::try_from(set_str.as_str())
            .map_err(|e| StoreError::InvalidInput(format!("invalid sequence set: {e}")))?;
        let mailbox = Mailbox::try_from(dest.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {dest}: {e}")))?;

        if has_move {
            let body = CommandBody::Move {
                sequence_set: sequence_set.clone(),
                mailbox: mailbox.clone(),
                uid: true,
            };
            if let Err(e) = self.execute(body).await {
                log::warn!("imap: UID MOVE failed ({e}), falling back to COPY + STORE + EXPUNGE");
                let body = CommandBody::copy(sequence_set, mailbox, true)
                    .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
                self.execute(body).await?;
                self.uid_store_flags(uids, StoreType::Add, vec![Flag::Deleted])
                    .await?;
                self.expunge().await?;
            }
        } else {
            let body = CommandBody::copy(sequence_set, mailbox, true)
                .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
            self.execute(body).await?;
            self.uid_store_flags(uids, StoreType::Add, vec![Flag::Deleted])
                .await?;
            self.expunge().await?;
        }
        Ok(())
    }

    /// EXPUNGE.
    pub async fn expunge(&mut self) -> Result<()> {
        self.execute(CommandBody::Expunge).await?;
        Ok(())
    }

    /// APPEND.
    pub async fn append(
        &mut self,
        folder: &str,
        raw: &[u8],
        flags: Vec<Flag<'static>>,
    ) -> Result<()> {
        let mailbox = Mailbox::try_from(folder.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {folder}: {e}")))?;
        let literal = Literal::try_from(raw.to_vec())
            .map_err(|e| StoreError::InvalidInput(format!("literal error: {e}")))?;

        let body = CommandBody::Append {
            mailbox,
            flags,
            date: None,
            message: LiteralOrLiteral8::Literal(literal),
        };
        self.execute(body).await?;
        Ok(())
    }

    /// CREATE mailbox.
    pub async fn create_folder(&mut self, folder: &str) -> Result<()> {
        let mailbox = Mailbox::try_from(folder.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {folder}: {e}")))?;
        let body = CommandBody::create(mailbox)
            .map_err(|e| StoreError::InvalidInput(format!("create args: {e}")))?;
        self.execute(body).await?;
        Ok(())
    }

    /// LIST folders.
    pub async fn list(&mut self, reference: &str, pattern: &str) -> Result<Vec<DiscoveredFolder>> {
        let body = CommandBody::list(reference.to_string(), pattern.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("list args: {e}")))?;

        let res = self.execute(body).await?;
        let mut out = Vec::new();

        for d in res.data {
            if let Data::List {
                items,
                delimiter,
                mailbox,
            } = d
            {
                let name = match mailbox {
                    Mailbox::Inbox => "INBOX".to_string(),
                    Mailbox::Other(o) => String::from_utf8_lossy(o.as_ref()).to_string(),
                };
                let delim = delimiter
                    .map(|d| d.inner().to_string())
                    .unwrap_or_else(|| "/".to_string());
                out.push(DiscoveredFolder {
                    name,
                    delimiter: delim,
                    attributes: items.into_iter().map(|i| i.into_static()).collect(),
                });
            }
        }
        Ok(out)
    }

    /// LSUB folders.
    pub async fn lsub(&mut self, reference: &str, pattern: &str) -> Result<Vec<DiscoveredFolder>> {
        let body = CommandBody::lsub(reference.to_string(), pattern.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("lsub args: {e}")))?;

        let res = self.execute(body).await?;
        let mut out = Vec::new();

        for d in res.data {
            if let Data::Lsub {
                items,
                delimiter,
                mailbox,
            } = d
            {
                let name = match mailbox {
                    Mailbox::Inbox => "INBOX".to_string(),
                    Mailbox::Other(o) => String::from_utf8_lossy(o.as_ref()).to_string(),
                };
                let delim = delimiter
                    .map(|d| d.inner().to_string())
                    .unwrap_or_else(|| "/".to_string());
                out.push(DiscoveredFolder {
                    name,
                    delimiter: delim,
                    attributes: items.into_iter().map(|i| i.into_static()).collect(),
                });
            }
        }
        Ok(out)
    }

    /// NOOP health check.
    pub async fn noop(&mut self) -> Result<()> {
        self.execute(CommandBody::Noop).await?;
        Ok(())
    }
}

/// Outcome of trashing a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashOutcome {
    Moved(String),
    Expunged,
}

/// Outcome of moving a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveOutcome {
    Moved(String),
    AlreadyThere,
}

/// Outcome of archiving a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveOutcome {
    Moved(String),
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

/// High-level IMAP synchronization engine.
pub struct ImapSync {
    endpoint: ImapEndpoint,
    account: crate::models::Account,
    session: Option<ImapSession>,
}

impl ImapSync {
    pub fn new(account: &crate::models::Account) -> Self {
        Self {
            endpoint: endpoint_for(account),
            account: account.clone(),
            session: None,
        }
    }

    /// Connect and authenticate with the server.
    pub async fn connect(&mut self, password: &str) -> Result<()> {
        if self.session.is_some() {
            return Ok(());
        }
        if !self.endpoint.implicit_tls && !self.endpoint.starttls {
            let sec = self.account.imap_security.trim().to_ascii_lowercase();
            if sec != "plain" && sec != "none" {
                return Err(StoreError::InvalidInput(format!(
                    "refusing plaintext IMAP connection to {}: set security to 'tls' or 'starttls'",
                    self.endpoint.addr
                )));
            }
        }

        let tcp = TcpStream::connect(&self.endpoint.addr)
            .await
            .map_err(|e| StoreError::Network(format!("connect {}: {e}", self.endpoint.addr)))?;

        let (stream, client) = if self.endpoint.implicit_tls {
            let tls_connector = build_tls_connector()?;
            let server_name = ServerName::try_from(self.endpoint.host.clone()).map_err(|e| {
                StoreError::Network(format!("invalid server name {}: {e}", self.endpoint.host))
            })?;
            let tls = tls_connector.connect(server_name, tcp).await.map_err(|e| {
                StoreError::Network(format!(
                    "TLS handshake failed with {}: {e}",
                    self.endpoint.addr
                ))
            })?;
            let mut stream = Stream::tls(tokio_rustls::TlsStream::Client(tls));
            let mut client = Client::new(Options::default());
            ImapSession::read_greeting(&mut stream, &mut client).await?;
            (stream, client)
        } else {
            let mut stream = Stream::insecure(tcp);
            let mut client = Client::new(Options::default());
            ImapSession::read_greeting(&mut stream, &mut client).await?;

            if self.endpoint.starttls {
                let tag = Tag::try_from("A0001").unwrap();
                let handle = client
                    .enqueue_command(Command::new(tag.clone(), CommandBody::StartTLS).unwrap());
                loop {
                    let event = stream
                        .next(&mut client)
                        .await
                        .map_err(|e| StoreError::Network(format!("STARTTLS stream error: {e}")))?;
                    match event {
                        Event::StatusReceived {
                            status: Status::Tagged(tagged),
                        } => {
                            if tagged.tag == tag {
                                if tagged.body.kind == StatusKind::Ok {
                                    break;
                                } else {
                                    return Err(StoreError::Network(format!(
                                        "STARTTLS rejected: {}",
                                        tagged.body.text
                                    )));
                                }
                            }
                        }
                        Event::CommandRejected {
                            handle: h, status, ..
                        } if h == handle => {
                            return Err(StoreError::Network(format!(
                                "STARTTLS rejected: {status:?}"
                            )));
                        }
                        _ => {}
                    }
                }

                let tcp_stream: TcpStream = stream.into();
                let tls_connector = build_tls_connector()?;
                let server_name =
                    ServerName::try_from(self.endpoint.host.clone()).map_err(|e| {
                        StoreError::Network(format!(
                            "invalid server name {}: {e}",
                            self.endpoint.host
                        ))
                    })?;
                let tls = tls_connector
                    .connect(server_name, tcp_stream)
                    .await
                    .map_err(|e| {
                        StoreError::Network(format!(
                            "STARTTLS handshake failed with {}: {e}",
                            self.endpoint.addr
                        ))
                    })?;
                let stream = Stream::tls(tokio_rustls::TlsStream::Client(tls));
                (stream, client)
            } else {
                (stream, client)
            }
        };

        let mut session = ImapSession {
            stream,
            client,
            tag_counter: 1,
            capabilities: Vec::new(),
            condstore_enabled: false,
            qresync_enabled: false,
        };

        let username = if self.account.imap_username.is_empty() {
            &self.account.email_address
        } else {
            &self.account.imap_username
        };

        session.login(username, password).await?;
        session.capability().await?;
        let _ = session.enable_extensions().await;

        self.session = Some(session);
        log::info!(
            "imap: connected and authenticated for {}",
            self.account.email_address
        );
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.session = None;
    }

    pub async fn reconnect(&mut self) -> Result<()> {
        self.session = None;
        let secrets = crate::auth::load_account_secrets(&self.account.auth_vault_key)
            .map_err(|e| StoreError::NotFound(format!("keyring secret: {e}")))?;
        self.connect(&secrets.imap_password).await
    }

    pub fn is_healthy(&self) -> bool {
        self.session.is_some()
    }

    fn session(&mut self) -> Result<&mut ImapSession> {
        self.session
            .as_mut()
            .ok_or_else(|| StoreError::Network("session disconnected".to_string()))
    }

    pub async fn capabilities_list(&mut self) -> Result<Vec<String>> {
        let session = self.session()?;
        session.capability().await
    }

    pub async fn search_server_into_cache(
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
            .filter(|f| folder_scope.map(|s| f.path == s).unwrap_or(true))
            .collect();
        for folder in targets {
            if report.fetched >= TOTAL_CAP {
                break;
            }
            let session = self.session()?;
            if session.select(&folder.path, None).await.is_err() {
                log::debug!("search: cannot select {}", folder.path);
                continue;
            }
            report.folders_searched += 1;
            let mut hits: Option<HashSet<u32>> = None;
            let mut failed = false;
            for tok in &ascii {
                let astring = match AString::try_from(tok.to_string()) {
                    Ok(a) => a,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                };
                match session.uid_search(vec1![SearchKey::Text(astring)]).await {
                    Ok(uids) => {
                        let set: HashSet<u32> = uids.into_iter().collect();
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
                let fetched = session.uid_fetch_messages(chunk).await?;
                for (uid, flags, raw) in fetched {
                    let (parsed, files) =
                        parse_to_new(account.id, folder.id, uid, &flags, &raw, false)?;
                    let id = messages::upsert(db, &parsed)?;
                    collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
                    store_attachment_meta(db, id, files);
                    report.fetched += 1;
                }
            }
        }
        Ok(report)
    }

    pub async fn append_to_folder(&mut self, folder_path: &str, raw: &[u8]) -> Result<()> {
        let session = self.session()?;
        session.append(folder_path, raw, vec![Flag::Seen]).await
    }

    pub async fn append_draft(&mut self, folder_path: &str, raw: &[u8]) -> Result<()> {
        let session = self.session()?;
        session
            .append(folder_path, raw, vec![Flag::Draft, Flag::Seen])
            .await
    }

    pub async fn trash_message(&mut self, db: &Db, message_id: i64) -> Result<TrashOutcome> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;
        let trash = folders::list_by_account(db, message.account_id)?
            .into_iter()
            .find(|f| f.role == FolderRole::Trash);

        let Some(trash) = trash.filter(|t| t.id != folder.id) else {
            self.delete_message(db, message_id).await?;
            return Ok(TrashOutcome::Expunged);
        };

        let session = self.session()?;
        session.select(&folder.path, None).await?;
        if let Err(e) = session
            .uid_store_flags(&[message.uid], StoreType::Add, vec![Flag::Seen])
            .await
        {
            log::warn!("imap: mark-seen before trash move failed: {e}");
        }
        session.uid_move(&[message.uid], &trash.path).await?;
        messages::delete(db, message_id)?;
        Ok(TrashOutcome::Moved(trash.path))
    }

    pub async fn archive_message(&mut self, db: &Db, message_id: i64) -> Result<ArchiveOutcome> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;
        let archive = folders::list_by_account(db, message.account_id)?
            .into_iter()
            .find(|f| f.role == FolderRole::Archive);

        let archive = match archive {
            Some(a) => a,
            None => {
                let delim = folders::list_by_account(db, message.account_id)?
                    .first()
                    .map(|f| f.delimiter.clone())
                    .unwrap_or_else(|| "/".to_string());
                self.create_folder_path(db, message.account_id, "Archive", &delim)
                    .await?
            }
        };

        if archive.id == folder.id {
            return Ok(ArchiveOutcome::AlreadyThere);
        }

        let session = self.session()?;
        session.select(&folder.path, None).await?;
        session.uid_move(&[message.uid], &archive.path).await?;
        messages::delete(db, message_id)?;
        Ok(ArchiveOutcome::Moved(archive.path))
    }

    pub async fn move_to_folder(
        &mut self,
        db: &Db,
        message_id: i64,
        dest_folder_id: i64,
    ) -> Result<MoveOutcome> {
        let msg = messages::get(db, message_id)?;
        if msg.folder_id == dest_folder_id {
            return Ok(MoveOutcome::AlreadyThere);
        }
        let src_folder = folders::get(db, msg.folder_id)?;
        let dest_folder = folders::get(db, dest_folder_id)?;

        let session = self.session()?;
        session.select(&src_folder.path, None).await?;
        if dest_folder.role == FolderRole::Trash {
            if let Err(e) = session
                .uid_store_flags(&[msg.uid], StoreType::Add, vec![Flag::Seen])
                .await
            {
                log::warn!("imap: mark-seen before move to trash failed: {e}");
            }
        }
        session.uid_move(&[msg.uid], &dest_folder.path).await?;
        messages::delete(db, message_id)?;
        Ok(MoveOutcome::Moved(dest_folder.path))
    }

    pub async fn create_folder_path(
        &mut self,
        db: &Db,
        account_id: i64,
        path: &str,
        delimiter: &str,
    ) -> Result<Folder> {
        let session = self.session()?;
        if let Err(e) = session.create_folder(path).await {
            let msg = e.to_string().to_ascii_lowercase();
            if !msg.contains("already exists") && !msg.contains("alreadyexists") {
                return Err(e);
            }
        }
        let id = folders::upsert(db, account_id, path, delimiter, FolderRole::Custom)?;
        folders::get(db, id)
    }

    pub async fn delete_message(&mut self, db: &Db, message_id: i64) -> Result<()> {
        let msg = messages::get(db, message_id)?;
        let folder = folders::get(db, msg.folder_id)?;
        let session = self.session()?;
        session.select(&folder.path, None).await?;
        session
            .uid_store_flags(&[msg.uid], StoreType::Add, vec![Flag::Deleted])
            .await?;
        session.expunge().await?;
        messages::delete(db, message_id)?;
        Ok(())
    }

    pub async fn move_uids_to(
        &mut self,
        db: &Db,
        src_folder_id: i64,
        uids: &[u32],
        dest_path: &str,
    ) -> Result<u64> {
        let mut clean: Vec<u32> = uids.to_vec();
        clean.sort_unstable();
        clean.dedup();
        if clean.is_empty() {
            return Ok(0);
        }
        let src = folders::get(db, src_folder_id)?;
        if src.path == dest_path {
            return Ok(0);
        }
        let trash = folders::list_by_account(db, src.account_id)?
            .into_iter()
            .find(|f| f.role == FolderRole::Trash);
        let dest_is_trash = trash.as_ref().is_some_and(|t| t.path == dest_path);

        let session = self.session()?;
        session.select(&src.path, None).await?;
        if dest_is_trash {
            if let Err(e) = session
                .uid_store_flags(&clean, StoreType::Add, vec![Flag::Seen])
                .await
            {
                log::warn!("imap: mark-seen before bulk trash move failed: {e}");
            }
        }
        session.uid_move(&clean, dest_path).await?;
        let count = messages::delete_many_by_uids(db, src_folder_id, &clean)?;
        Ok(count)
    }

    pub async fn purge_uids(&mut self, db: &Db, folder_id: i64, uids: &[u32]) -> Result<u64> {
        if uids.is_empty() {
            return Ok(0);
        }
        let folder = folders::get(db, folder_id)?;
        let session = self.session()?;
        session.select(&folder.path, None).await?;
        session
            .uid_store_flags(uids, StoreType::Add, vec![Flag::Deleted])
            .await?;
        session.expunge().await?;
        let count = messages::delete_many_by_uids(db, folder_id, uids)?;
        Ok(count)
    }

    /// Synchronize a folder window using CONDSTORE / QRESYNC delta sync when supported.
    pub async fn sync_folder_window(
        &mut self,
        db: &Db,
        folder_id: i64,
        window: Option<usize>,
    ) -> Result<SyncReport> {
        let folder = folders::get(db, folder_id)?;
        let account = accounts::get(db, folder.account_id)?;
        let session = self.session()?;

        let qresync_param = folder.uid_validity.map(|v| (v, folder.highest_modseq));
        let mb = session.select(&folder.path, qresync_param).await?;
        log::info!(
            "imap: SELECT {} ({} mails, uid_next {:?}, modseq {:?}, vanished: {})",
            folder.path,
            mb.exists,
            mb.uid_next,
            mb.highest_modseq,
            mb.vanished.len()
        );

        // UIDVALIDITY change => server-side rebuild, drop local copies.
        if let Some(validity) = mb.uid_validity {
            if folder.uid_validity.is_some_and(|v| v != validity) {
                log::warn!("imap: UIDVALIDITY changed for {} — resyncing", folder.path);
                messages::delete_by_folder(db, folder_id)?;
            }
        }

        let mut expunged = 0u64;

        // 1. Process QRESYNC VANISHED UIDs immediately if reported by server.
        if !mb.vanished.is_empty() {
            log::info!(
                "imap: QRESYNC reported {} vanished UIDs in {}",
                mb.vanished.len(),
                folder.path
            );
            for uid in &mb.vanished {
                messages::delete_by_uid(db, folder_id, *uid)?;
                expunged += 1;
            }
        }

        let local_uids: HashSet<u32> = messages::list_uids(db, folder_id)?.into_iter().collect();
        let (server_uids, search_lo) = search_recent_uids(session, window, mb.uid_next).await?;

        // Newest-N relevance window: UIDs grow monotonically, so the largest
        // N are the newest. Everything outside costs no network.
        let relevant: Option<HashSet<u32>> = window.map(|n| {
            let mut sorted: Vec<u32> = server_uids.iter().copied().collect();
            sorted.sort_unstable();
            let skip = sorted.len().saturating_sub(n);
            sorted.into_iter().skip(skip).collect()
        });
        let in_window = |uid: &u32| relevant.as_ref().is_none_or(|r| r.contains(uid));
        let is_trash = folder.role == FolderRole::Trash;

        // 2. Flag refresh for messages we already have within the window.
        // This guarantees that whatever messages are currently in view have 100%
        // accurate flags and unread counts matching the server.
        let existing: Vec<u32> = server_uids
            .intersection(&local_uids)
            .copied()
            .filter(in_window)
            .collect();
        for chunk in existing.chunks(FETCH_CHUNK) {
            let changed = session.uid_fetch_flags_changesince(chunk, 0).await?;
            for (uid, flags, _) in changed {
                let (read, starred, draft) = flag_state(&flags);
                let read = read || is_trash;
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

        // 3. If CONDSTORE is enabled, also check for flag changes on older local messages
        // that fall outside the active window using CHANGEDSINCE.
        if session.condstore_enabled && folder.highest_modseq > 0 {
            let older_existing: Vec<u32> = local_uids
                .iter()
                .copied()
                .filter(|u| !in_window(u))
                .collect();
            for chunk in older_existing.chunks(FETCH_CHUNK) {
                let changed = session
                    .uid_fetch_flags_changesince(chunk, folder.highest_modseq)
                    .await?;
                for (uid, flags, _) in changed {
                    let (read, starred, draft) = flag_state(&flags);
                    let read = read || is_trash;
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

        // 4. Full fetch of new messages (windowed). BODY.PEEK[] is mandatory here.
        let mut fetched = 0u64;
        let mut missing: Vec<u32> = server_uids
            .difference(&local_uids)
            .copied()
            .filter(in_window)
            .collect();
        missing.sort_unstable();

        for chunk in missing.chunks(FETCH_CHUNK) {
            let messages_data = session.uid_fetch_messages(chunk).await?;
            for (uid, flags, raw) in messages_data {
                let (mut parsed, files) =
                    parse_to_new(account.id, folder_id, uid, &flags, &raw, false)?;
                if is_trash {
                    parsed.is_read = true;
                }
                let id = messages::upsert(db, &parsed)?;
                collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
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

        // 5. Expunge locally what the searched UID range no longer has.
        // UIDs below `search_lo` were never asked about, so they stay cached.
        // This diffing runs locally at 0 network cost to catch server deletions.
        for uid in &local_uids {
            if *uid >= search_lo && !server_uids.contains(uid) {
                messages::delete_by_uid(db, folder_id, *uid)?;
                expunged += 1;
            }
        }

        // 6. If this is Trash, ensure any unread messages in local DB are marked \Seen on server.
        if is_trash {
            if let Ok(unread_uids) = messages::list_unread_uids(db, folder_id) {
                if !unread_uids.is_empty() {
                    let _ = session
                        .uid_store_flags(&unread_uids, StoreType::Add, vec![Flag::Seen])
                        .await;
                    for uid in &unread_uids {
                        let _ = messages::set_flags_by_uid(
                            db,
                            account.id,
                            folder_id,
                            *uid,
                            true,
                            false,
                            false,
                        );
                    }
                }
            }
        }

        let validity = mb.uid_validity.unwrap_or(folder.uid_validity.unwrap_or(0));
        let uid_next = mb.uid_next.unwrap_or(folder.uid_next.unwrap_or(0));
        let new_modseq = mb.highest_modseq.unwrap_or(folder.highest_modseq);
        folders::set_sync_state(
            db,
            folder_id,
            validity,
            uid_next,
            u64::from(mb.exists),
            new_modseq,
        )?;

        Ok(SyncReport {
            fetched,
            expunged,
            folders: 0,
        })
    }

    pub async fn sync_older(
        &mut self,
        db: &Db,
        folder_id: i64,
        batch: usize,
    ) -> Result<SyncReport> {
        let folder = folders::get(db, folder_id)?;
        let account = accounts::get(db, folder.account_id)?;
        let session = self.session()?;

        let mb = session.select(&folder.path, None).await?;
        if let Some(validity) = mb.uid_validity {
            if folder.uid_validity.is_some_and(|v| v != validity) {
                return self
                    .sync_folder_window(db, folder_id, Some(FULL_SYNC_WINDOW))
                    .await;
            }
        }

        let min_uid = match messages::min_uid(db, folder_id)? {
            Some(u) => u,
            None => {
                return self
                    .sync_folder_window(db, folder_id, Some(FULL_SYNC_WINDOW))
                    .await;
            }
        };

        if min_uid <= 1 {
            return Ok(SyncReport::default());
        }

        let hi = min_uid.saturating_sub(1);
        let seq = SequenceSet::try_from(format!("1:{hi}").as_str())
            .map_err(|e| StoreError::InvalidInput(format!("seq: {e}")))?;
        let mut server_uids = session.uid_search(vec1![SearchKey::Uid(seq)]).await?;
        server_uids.sort_unstable();

        let local_uids: HashSet<u32> = messages::list_uids(db, folder_id)?.into_iter().collect();
        let missing: Vec<u32> = server_uids
            .into_iter()
            .rev()
            .filter(|u| !local_uids.contains(u))
            .take(batch)
            .collect();

        let mut fetched = 0u64;
        let mut missing_sorted = missing;
        missing_sorted.sort_unstable();

        for chunk in missing_sorted.chunks(FETCH_CHUNK) {
            let messages_data = session.uid_fetch_messages(chunk).await?;
            for (uid, flags, raw) in messages_data {
                let (parsed, files) =
                    parse_to_new(account.id, folder_id, uid, &flags, &raw, false)?;
                let id = messages::upsert(db, &parsed)?;
                collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
                store_attachment_meta(db, id, files);
                fetched += 1;
            }
        }

        let validity = mb.uid_validity.unwrap_or(folder.uid_validity.unwrap_or(0));
        let uid_next = mb.uid_next.unwrap_or(folder.uid_next.unwrap_or(0));
        let modseq = mb.highest_modseq.unwrap_or(folder.highest_modseq);
        folders::set_sync_state(
            db,
            folder_id,
            validity,
            uid_next,
            u64::from(mb.exists),
            modseq,
        )?;

        Ok(SyncReport {
            fetched,
            expunged: 0,
            folders: 0,
        })
    }

    pub async fn fetch_attachments(&mut self, db: &Db, message_id: i64) -> Result<u64> {
        let msg = messages::get(db, message_id)?;
        let folder = folders::get(db, msg.folder_id)?;
        let session = self.session()?;
        session.select(&folder.path, None).await?;

        let fetched = session.uid_fetch_messages(&[msg.uid]).await?;
        let raw = match fetched.into_iter().next() {
            Some((_, _, r)) => r,
            None => return Err(StoreError::NotFound(format!("message uid {}", msg.uid))),
        };

        let parsed = mail_parser::MessageParser::default()
            .parse(&raw)
            .ok_or_else(|| StoreError::InvalidInput("parse failed".to_string()))?;
        let files = extract_attachments(&parsed, true);
        let stored = files.len() as u64;
        store_attachments(db, message_id, files)?;
        Ok(stored)
    }
}

impl SyncProvider for ImapSync {
    fn name(&self) -> &'static str {
        "imap"
    }

    async fn sync_folders(&mut self, db: &Db, account_id: i64) -> Result<Vec<Folder>> {
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

        let session = self.session()?;

        // Pass 1: LIST "" "*"
        let names = session.list("", "*").await?;
        for n in &names {
            if !is_selectable(&n.attributes) {
                continue;
            }
            consider(
                &n.name,
                &n.delimiter,
                map_folder_role(&n.attributes, &n.name),
                &attr_text(&n.attributes),
                "LIST",
            );
        }

        // Pass 2: LSUB "" "*"
        if let Ok(subs) = session.lsub("", "*").await {
            for n in &subs {
                if !is_selectable(&n.attributes) {
                    continue;
                }
                consider(
                    &n.name,
                    &n.delimiter,
                    role_from_name(&n.name),
                    &attr_text(&n.attributes),
                    "LSUB",
                );
            }
        }

        let mut out = Vec::new();
        for (path, delimiter, role) in &discovered {
            let id = folders::upsert(db, account_id, path, delimiter, *role)?;
            out.push(folders::get(db, id)?);
        }
        Ok(out)
    }

    async fn sync_folder(&mut self, db: &Db, folder_id: i64) -> Result<SyncReport> {
        self.sync_folder_window(db, folder_id, Some(FULL_SYNC_WINDOW))
            .await
    }

    async fn push_flags(&mut self, db: &Db, message: &Message) -> Result<()> {
        let folder = folders::get(db, message.folder_id)?;
        let session = self.session()?;
        session.select(&folder.path, None).await?;

        let seen_op = if message.is_read {
            StoreType::Add
        } else {
            StoreType::Remove
        };
        session
            .uid_store_flags(&[message.uid], seen_op, vec![Flag::Seen])
            .await?;

        let star_op = if message.is_starred {
            StoreType::Add
        } else {
            StoreType::Remove
        };
        session
            .uid_store_flags(&[message.uid], star_op, vec![Flag::Flagged])
            .await?;

        Ok(())
    }
}

/// SEARCH newest UIDs within window.
async fn search_recent_uids(
    session: &mut ImapSession,
    window: Option<usize>,
    uid_next: Option<u32>,
) -> Result<(HashSet<u32>, u32)> {
    let Some(n) = window else {
        let uids = session.uid_search(vec1![SearchKey::All]).await?;
        return Ok((uids.into_iter().collect(), 1));
    };

    let top = uid_next.unwrap_or(0).saturating_sub(1);
    if top == 0 {
        let uids = session.uid_search(vec1![SearchKey::All]).await?;
        return Ok((uids.into_iter().collect(), 1));
    }

    let span = (n as u32).saturating_mul(8).max(n as u32);
    search_paged(session, top, n, span).await
}

const SEARCH_PAGES: u32 = 8;

/// SEARCH UID space backwards from `top`, paging down until `want` UIDs are
/// known or UID 1 is reached.
async fn search_paged(
    session: &mut ImapSession,
    top: u32,
    want: usize,
    span: u32,
) -> Result<(HashSet<u32>, u32)> {
    let span = span.max(1);
    let mut found = HashSet::new();
    let mut hi = top;
    let mut lo = hi.saturating_sub(span.saturating_sub(1)).max(1);
    for _ in 0..SEARCH_PAGES {
        let seq_str = format!("{lo}:{hi}");
        let seq = SequenceSet::try_from(seq_str.as_str()).map_err(|e| {
            StoreError::InvalidInput(format!("invalid sequence set {seq_str}: {e}"))
        })?;
        let uids = session.uid_search(vec1![SearchKey::Uid(seq)]).await?;
        found.extend(uids);
        if found.len() >= want || lo <= 1 {
            break;
        }
        hi = lo.saturating_sub(1);
        if hi == 0 {
            break;
        }
        lo = hi.saturating_sub(span.saturating_sub(1)).max(1);
    }
    Ok((found, lo))
}

fn flag_state(flags: &[Flag<'static>]) -> (bool, bool, bool) {
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

fn parse_to_new(
    account_id: i64,
    folder_id: i64,
    uid: u32,
    flags: &[Flag<'static>],
    raw: &[u8],
    with_bytes: bool,
) -> Result<(NewMessage, Vec<NewAttachment>)> {
    let parsed = mail_parser::MessageParser::default()
        .parse(raw)
        .ok_or_else(|| StoreError::InvalidInput(format!("cannot parse message uid {uid}")))?;
    let (is_read, is_starred, is_draft) = flag_state(flags);
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
            continue;
        }
        if len == 0 {
            continue;
        }
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
        }
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

pub fn normalize_folder_path(input: &str, delimiter: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(StoreError::InvalidInput(
            "folder name cannot be empty".into(),
        ));
    }
    let parts: Vec<&str> = trimmed
        .split(delimiter)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(StoreError::InvalidInput("invalid folder path".into()));
    }
    Ok(parts.join(delimiter))
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
            imap_port: 993,
            imap_security: "starttls".to_string(),
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
        assert!(!ep.implicit_tls);
        assert!(ep.starttls);

        a.imap_port = 143;
        a.imap_security = "tls".to_string();
        let ep = endpoint_for(&a);
        assert!(ep.implicit_tls);
        assert!(!ep.starttls);
    }

    #[test]
    fn folder_path_normalization() {
        assert_eq!(normalize_folder_path("  INBOX  ", "/").unwrap(), "INBOX");
        assert_eq!(
            normalize_folder_path("INBOX/Archive/2026", "/").unwrap(),
            "INBOX/Archive/2026"
        );
        assert_eq!(
            normalize_folder_path("  INBOX . Archive . 2026  ", ".").unwrap(),
            "INBOX.Archive.2026"
        );
    }

    #[test]
    fn role_heuristics_cover_german_and_english_names() {
        assert_eq!(role_from_name("INBOX"), FolderRole::Inbox);
        assert_eq!(role_from_name("Sent"), FolderRole::Sent);
        assert_eq!(role_from_name("Gesendete Elemente"), FolderRole::Sent);
        assert_eq!(role_from_name("Entwürfe"), FolderRole::Drafts);
        assert_eq!(role_from_name("Papierkorb"), FolderRole::Trash);
        assert_eq!(role_from_name("Gelöschte Elemente"), FolderRole::Trash);
        assert_eq!(role_from_name("Spam"), FolderRole::Junk);
        assert_eq!(role_from_name("Archiv"), FolderRole::Archive);
    }

    #[test]
    fn fresh_session_is_not_healthy() {
        let a = crate::models::Account {
            id: 1,
            name: "n".to_string(),
            email_address: "e".to_string(),
            from_name: String::new(),
            imap_host: "h".to_string(),
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
        let s = ImapSync::new(&a);
        assert!(!s.is_healthy());
    }

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
    fn test_flag_state_mapping() {
        let (read, starred, draft) = flag_state(&[]);
        assert!(!read);
        assert!(!starred);
        assert!(!draft);

        let (read, starred, draft) = flag_state(&[Flag::Seen]);
        assert!(read);
        assert!(!starred);
        assert!(!draft);

        let (read, starred, draft) = flag_state(&[Flag::Seen, Flag::Flagged]);
        assert!(read);
        assert!(starred);
        assert!(!draft);

        let (read, starred, draft) = flag_state(&[Flag::Draft]);
        assert!(!read);
        assert!(!starred);
        assert!(draft);
    }

    #[test]
    fn test_search_window_in_window_logic() {
        let server_uids: HashSet<u32> = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10].into_iter().collect();
        let window = Some(5);
        let relevant: Option<HashSet<u32>> = window.map(|n| {
            let mut sorted: Vec<u32> = server_uids.iter().copied().collect();
            sorted.sort_unstable();
            let skip = sorted.len().saturating_sub(n);
            sorted.into_iter().skip(skip).collect()
        });
        let in_window = |uid: &u32| relevant.as_ref().is_none_or(|r| r.contains(uid));

        for uid in 1..=5 {
            assert!(!in_window(&uid));
        }
        for uid in 6..=10 {
            assert!(in_window(&uid));
        }
    }

    struct MockImapServer {
        port: u16,
        received: Arc<tokio::sync::Mutex<Vec<String>>>,
        _handle: tokio::task::JoinHandle<()>,
    }

    impl MockImapServer {
        async fn start<F>(caps: &'static str, custom_handler: F) -> Self
        where
            F: Fn(&str, &str) -> Vec<String> + Send + Sync + 'static,
        {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            use tokio::net::TcpListener;

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));
            let rec_clone = Arc::clone(&received);

            let handle = tokio::spawn(async move {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);

                let greeting = format!("* OK [CAPABILITY {caps}] Mock IMAP Server ready\r\n");
                if writer.write_all(greeting.as_bytes()).await.is_err() {
                    return;
                }

                let mut line = String::new();
                while let Ok(n) = reader.read_line(&mut line).await {
                    if n == 0 {
                        break;
                    }
                    let raw_cmd = line.trim_end_matches(['\r', '\n']).to_string();
                    line.clear();
                    if raw_cmd.is_empty() {
                        continue;
                    }

                    rec_clone.lock().await.push(raw_cmd.clone());

                    let mut parts = raw_cmd.splitn(2, ' ');
                    let tag = parts.next().unwrap_or("*");
                    let rest = parts.next().unwrap_or("");
                    let upper_rest = rest.to_ascii_uppercase();

                    let responses = if upper_rest.starts_with("LOGIN") {
                        vec![format!("{tag} OK LOGIN completed\r\n")]
                    } else if upper_rest.starts_with("CAPABILITY") {
                        vec![
                            format!("* CAPABILITY {caps}\r\n"),
                            format!("{tag} OK CAPABILITY completed\r\n"),
                        ]
                    } else if upper_rest.starts_with("ENABLE") {
                        if caps.contains("ENABLE") {
                            let enabled = rest.strip_prefix("ENABLE ").unwrap_or("").trim();
                            vec![
                                format!("* ENABLED {enabled}\r\n"),
                                format!("{tag} OK ENABLE completed\r\n"),
                            ]
                        } else {
                            vec![format!("{tag} BAD ENABLE unknown command\r\n")]
                        }
                    } else {
                        custom_handler(tag, rest)
                    };

                    for resp in responses {
                        if writer.write_all(resp.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                }
            });

            Self {
                port,
                received,
                _handle: handle,
            }
        }
    }

    fn test_mock_account(port: u16) -> crate::models::Account {
        crate::models::Account {
            id: 1,
            name: "Mock Account".to_string(),
            email_address: "alice@example.com".to_string(),
            from_name: "Alice".to_string(),
            imap_host: "127.0.0.1".to_string(),
            imap_port: port,
            imap_security: "plain".to_string(),
            imap_username: "alice@example.com".to_string(),
            smtp_host: "127.0.0.1".to_string(),
            smtp_port: 25,
            smtp_security: "plain".to_string(),
            smtp_username: "alice@example.com".to_string(),
            auth_vault_key: "vault_key".to_string(),
            check_interval_secs: 300,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[tokio::test]
    async fn test_mock_capabilities_guard_no_extensions() {
        let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
            let upper = rest.to_ascii_uppercase();
            if upper.starts_with("SELECT") {
                vec![
                    format!("* 10 EXISTS\r\n"),
                    format!("* OK [UIDVALIDITY 1] Ok\r\n"),
                    format!("* OK [UIDNEXT 100] Ok\r\n"),
                    format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
                ]
            } else if upper.starts_with("UID COPY") {
                vec![format!("{tag} OK UID COPY completed\r\n")]
            } else if upper.starts_with("UID STORE") {
                vec![format!("{tag} OK STORE completed\r\n")]
            } else if upper.starts_with("EXPUNGE") {
                vec![
                    format!("* 1 EXPUNGE\r\n"),
                    format!("{tag} OK EXPUNGE completed\r\n"),
                ]
            } else {
                vec![format!("{tag} OK completed\r\n")]
            }
        })
        .await;

        let account = test_mock_account(server.port);
        let mut sync = ImapSync::new(&account);
        sync.connect("secret").await.unwrap();

        {
            let session = sync.session().unwrap();
            assert!(session.has_capability("IMAP4rev1"));
            assert!(!session.condstore_enabled);
            assert!(!session.qresync_enabled);
            assert!(!session.has_capability("MOVE"));
            assert!(!session.has_capability("ENABLE"));
        }

        let session = sync.session.as_mut().unwrap();
        session.select("INBOX", None).await.unwrap();
        session.uid_move(&[10], "Trash").await.unwrap();

        let cmds = server.received.lock().await.clone();
        // Assert ENABLE was never sent
        assert!(!cmds.iter().any(|c| c.to_ascii_uppercase().contains("ENABLE")));
        // Assert SELECT was sent as standard SELECT without CONDSTORE or QRESYNC
        assert!(cmds
            .iter()
            .any(|c| c.contains("SELECT") && !c.contains("CONDSTORE") && !c.contains("QRESYNC")));
        // Assert UID MOVE was NEVER sent, but COPY + STORE \Deleted + EXPUNGE was used
        assert!(!cmds.iter().any(|c| c.to_ascii_uppercase().contains("UID MOVE")));
        assert!(cmds.iter().any(|c| c.to_ascii_uppercase().contains("UID COPY 10")));
        assert!(cmds.iter().any(|c| c.contains("STORE") && c.contains("\\Deleted")));
        assert!(cmds.iter().any(|c| c.to_ascii_uppercase().contains("EXPUNGE")));
    }

    #[tokio::test]
    async fn test_mock_select_fallback_on_unsupported_condstore() {
        let server = MockImapServer::start("IMAP4rev1 CONDSTORE", |tag, rest| {
            let upper = rest.to_ascii_uppercase();
            if upper.contains("(CONDSTORE)") {
                // Server rejects the extension parameter
                vec![format!("{tag} BAD [CANNOT] parameter not supported\r\n")]
            } else if upper.starts_with("SELECT") {
                vec![
                    format!("* 5 EXISTS\r\n"),
                    format!("* OK [UIDVALIDITY 1] Ok\r\n"),
                    format!("* OK [UIDNEXT 50] Ok\r\n"),
                    format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
                ]
            } else {
                vec![format!("{tag} OK completed\r\n")]
            }
        })
        .await;

        let account = test_mock_account(server.port);
        let mut sync = ImapSync::new(&account);
        sync.connect("secret").await.unwrap();

        let session = sync.session.as_mut().unwrap();
        assert!(session.condstore_enabled);

        let sel = session.select("INBOX", None).await.unwrap();
        assert_eq!(sel.exists, 5);
        // condstore should now be disabled due to the fallback
        assert!(!session.condstore_enabled);

        let cmds = server.received.lock().await.clone();
        // First SELECT attempted with CONDSTORE
        assert!(cmds.iter().any(|c| c.contains("SELECT") && c.contains("CONDSTORE")));
        // Second SELECT fell back to standard SELECT
        assert!(cmds.iter().any(|c| c.contains("SELECT") && !c.contains("CONDSTORE")));
    }

    #[tokio::test]
    async fn test_mock_uid_move_fallback_when_server_rejects_move() {
        let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
            let upper = rest.to_ascii_uppercase();
            if upper.starts_with("UID MOVE") {
                // Server advertised MOVE but failed the command
                vec![format!("{tag} NO [CANNOT] UID MOVE not supported on this folder\r\n")]
            } else if upper.starts_with("UID COPY") {
                vec![format!("{tag} OK UID COPY completed\r\n")]
            } else if upper.starts_with("UID STORE") {
                vec![format!("{tag} OK STORE completed\r\n")]
            } else if upper.starts_with("EXPUNGE") {
                vec![
                    format!("* 1 EXPUNGE\r\n"),
                    format!("{tag} OK EXPUNGE completed\r\n"),
                ]
            } else {
                vec![format!("{tag} OK completed\r\n")]
            }
        })
        .await;

        let account = test_mock_account(server.port);
        let mut sync = ImapSync::new(&account);
        sync.connect("secret").await.unwrap();

        let session = sync.session.as_mut().unwrap();
        assert!(session.has_capability("MOVE"));

        session.uid_move(&[42], "Trash").await.unwrap();

        let cmds = server.received.lock().await.clone();
        // UID MOVE was tried first
        assert!(cmds.iter().any(|c| c.to_ascii_uppercase().contains("UID MOVE 42")));
        // Fallback sequence was executed
        assert!(cmds.iter().any(|c| c.to_ascii_uppercase().contains("UID COPY 42")));
        assert!(cmds.iter().any(|c| c.contains("STORE") && c.contains("\\Deleted")));
        assert!(cmds.iter().any(|c| c.to_ascii_uppercase().contains("EXPUNGE")));
    }

    #[tokio::test]
    async fn test_mock_changesince_fallback_when_server_rejects_modifier() {
        let server = MockImapServer::start("IMAP4rev1 CONDSTORE", |tag, rest| {
            let upper = rest.to_ascii_uppercase();
            if upper.contains("CHANGEDSINCE") {
                vec![format!("{tag} BAD Unknown modifier CHANGEDSINCE\r\n")]
            } else if upper.starts_with("UID FETCH") {
                vec![
                    format!("* 1 FETCH (UID 7 FLAGS (\\Seen))\r\n"),
                    format!("{tag} OK UID FETCH completed\r\n"),
                ]
            } else {
                vec![format!("{tag} OK completed\r\n")]
            }
        })
        .await;

        let account = test_mock_account(server.port);
        let mut sync = ImapSync::new(&account);
        sync.connect("secret").await.unwrap();

        let session = sync.session.as_mut().unwrap();
        assert!(session.condstore_enabled);

        let res = session.uid_fetch_flags_changesince(&[7], 100).await.unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].0, 7);
        assert_eq!(res[0].1, vec![Flag::Seen]);
        // condstore should now be disabled due to fallback
        assert!(!session.condstore_enabled);

        let cmds = server.received.lock().await.clone();
        assert!(cmds.iter().any(|c| c.contains("CHANGEDSINCE")));
        assert!(cmds.iter().any(|c| c.contains("UID FETCH") && !c.contains("CHANGEDSINCE")));
    }

    #[tokio::test]
    async fn test_mock_trash_message_marks_seen_before_move() {
        let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
            let upper = rest.to_ascii_uppercase();
            if upper.starts_with("SELECT") {
                vec![
                    format!("* 1 EXISTS\r\n"),
                    format!("* OK [UIDVALIDITY 1] Ok\r\n"),
                    format!("* OK [UIDNEXT 100] Ok\r\n"),
                    format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
                ]
            } else if upper.starts_with("UID STORE") {
                vec![
                    format!("* 1 FETCH (UID 99 FLAGS (\\Seen))\r\n"),
                    format!("{tag} OK STORE completed\r\n"),
                ]
            } else if upper.starts_with("UID MOVE") {
                vec![format!("{tag} OK UID MOVE completed\r\n")]
            } else {
                vec![format!("{tag} OK completed\r\n")]
            }
        })
        .await;

        let db = Db::open_in_memory().unwrap();
        let account = test_mock_account(server.port);
        let account_id = accounts::create(
            &db,
            &crate::models::NewAccount {
                name: account.name.clone(),
                email_address: account.email_address.clone(),
                from_name: account.from_name.clone(),
                imap_host: account.imap_host.clone(),
                imap_port: account.imap_port,
                imap_security: account.imap_security.clone(),
                imap_username: account.imap_username.clone(),
                smtp_host: account.smtp_host.clone(),
                smtp_port: account.smtp_port,
                smtp_security: account.smtp_security.clone(),
                smtp_username: account.smtp_username.clone(),
                auth_vault_key: account.auth_vault_key.clone(),
                check_interval_secs: account.check_interval_secs,
            },
        )
        .unwrap();

        let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        let _trash_id = folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();

        // Create an unread message
        let mut new_msg = messages::sample_new(account_id, inbox_id, 99);
        new_msg.is_read = false;
        let msg_id = messages::upsert(&db, &new_msg).unwrap();

        let mut sync = ImapSync::new(&account);
        sync.connect("secret").await.unwrap();

        let outcome = sync.trash_message(&db, msg_id).await.unwrap();
        assert_eq!(outcome, TrashOutcome::Moved("Trash".to_string()));

        let cmds = server.received.lock().await.clone();
        let store_idx = cmds.iter().position(|c| c.contains("STORE") && c.contains("\\Seen"));
        let move_idx = cmds.iter().position(|c| c.contains("UID MOVE"));

        assert!(store_idx.is_some(), "UID STORE \\Seen was not called for unread message");
        assert!(move_idx.is_some(), "UID MOVE was not called");
        assert!(
            store_idx.unwrap() < move_idx.unwrap(),
            "UID STORE \\Seen must occur BEFORE UID MOVE"
        );
    }

    #[tokio::test]
    async fn test_mock_trash_message_always_marks_seen() {
        let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
            let upper = rest.to_ascii_uppercase();
            if upper.starts_with("SELECT") {
                vec![
                    format!("* 1 EXISTS\r\n"),
                    format!("* OK [UIDVALIDITY 1] Ok\r\n"),
                    format!("* OK [UIDNEXT 100] Ok\r\n"),
                    format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
                ]
            } else if upper.starts_with("UID STORE") {
                vec![format!("{tag} OK UID STORE completed\r\n")]
            } else if upper.starts_with("UID MOVE") {
                vec![format!("{tag} OK UID MOVE completed\r\n")]
            } else {
                vec![format!("{tag} OK completed\r\n")]
            }
        })
        .await;

        let db = Db::open_in_memory().unwrap();
        let account = test_mock_account(server.port);
        let account_id = accounts::create(
            &db,
            &crate::models::NewAccount {
                name: account.name.clone(),
                email_address: account.email_address.clone(),
                from_name: account.from_name.clone(),
                imap_host: account.imap_host.clone(),
                imap_port: account.imap_port,
                imap_security: account.imap_security.clone(),
                imap_username: account.imap_username.clone(),
                smtp_host: account.smtp_host.clone(),
                smtp_port: account.smtp_port,
                smtp_security: account.smtp_security.clone(),
                smtp_username: account.smtp_username.clone(),
                auth_vault_key: account.auth_vault_key.clone(),
                check_interval_secs: account.check_interval_secs,
            },
        )
        .unwrap();

        let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        let _trash_id = folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();

        // Create an ALREADY-READ message
        let mut new_msg = messages::sample_new(account_id, inbox_id, 99);
        new_msg.is_read = true;
        let msg_id = messages::upsert(&db, &new_msg).unwrap();

        let mut sync = ImapSync::new(&account);
        sync.connect("secret").await.unwrap();

        let outcome = sync.trash_message(&db, msg_id).await.unwrap();
        assert_eq!(outcome, TrashOutcome::Moved("Trash".to_string()));

        let cmds = server.received.lock().await.clone();
        let store_idx = cmds.iter().position(|c| c.contains("STORE") && c.contains("\\Seen"));
        let move_idx = cmds.iter().position(|c| c.contains("UID MOVE"));

        assert!(
            store_idx.is_some(),
            "UID STORE \\Seen must be called when trashing to guarantee message is seen"
        );
        assert!(move_idx.is_some(), "UID MOVE was not called");
        assert!(
            store_idx.unwrap() < move_idx.unwrap(),
            "UID STORE \\Seen must occur BEFORE UID MOVE"
        );
    }

    #[tokio::test]
    async fn test_mock_move_to_folder_trash_marks_seen() {
        let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
            let upper = rest.to_ascii_uppercase();
            if upper.starts_with("SELECT") {
                vec![
                    format!("* 1 EXISTS\r\n"),
                    format!("* OK [UIDVALIDITY 1] Ok\r\n"),
                    format!("* OK [UIDNEXT 100] Ok\r\n"),
                    format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
                ]
            } else if upper.starts_with("UID STORE") {
                vec![format!("{tag} OK UID STORE completed\r\n")]
            } else if upper.starts_with("UID MOVE") {
                vec![format!("{tag} OK UID MOVE completed\r\n")]
            } else {
                vec![format!("{tag} OK completed\r\n")]
            }
        })
        .await;

        let db = Db::open_in_memory().unwrap();
        let account = test_mock_account(server.port);
        let account_id = accounts::create(
            &db,
            &crate::models::NewAccount {
                name: account.name.clone(),
                email_address: account.email_address.clone(),
                from_name: account.from_name.clone(),
                imap_host: account.imap_host.clone(),
                imap_port: account.imap_port,
                imap_security: account.imap_security.clone(),
                imap_username: account.imap_username.clone(),
                smtp_host: account.smtp_host.clone(),
                smtp_port: account.smtp_port,
                smtp_security: account.smtp_security.clone(),
                smtp_username: account.smtp_username.clone(),
                auth_vault_key: account.auth_vault_key.clone(),
                check_interval_secs: account.check_interval_secs,
            },
        )
        .unwrap();

        let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        let trash_id = folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();

        let new_msg = messages::sample_new(account_id, inbox_id, 88);
        let msg_id = messages::upsert(&db, &new_msg).unwrap();

        let mut sync = ImapSync::new(&account);
        sync.connect("secret").await.unwrap();

        let outcome = sync.move_to_folder(&db, msg_id, trash_id).await.unwrap();
        assert_eq!(outcome, MoveOutcome::Moved("Trash".to_string()));

        let cmds = server.received.lock().await.clone();
        let store_idx = cmds.iter().position(|c| c.contains("STORE") && c.contains("\\Seen"));
        let move_idx = cmds.iter().position(|c| c.contains("UID MOVE"));

        assert!(
            store_idx.is_some(),
            "UID STORE \\Seen must be called in move_to_folder when target is Trash"
        );
        assert!(move_idx.is_some(), "UID MOVE was not called");
        assert!(
            store_idx.unwrap() < move_idx.unwrap(),
            "UID STORE \\Seen must occur BEFORE UID MOVE"
        );
    }

    #[tokio::test]
    async fn test_mock_move_uids_to_trash_marks_seen() {
        let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
            let upper = rest.to_ascii_uppercase();
            if upper.starts_with("SELECT") {
                vec![
                    format!("* 2 EXISTS\r\n"),
                    format!("* OK [UIDVALIDITY 1] Ok\r\n"),
                    format!("* OK [UIDNEXT 100] Ok\r\n"),
                    format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
                ]
            } else if upper.starts_with("UID STORE") {
                vec![format!("{tag} OK STORE completed\r\n")]
            } else if upper.starts_with("UID MOVE") {
                vec![format!("{tag} OK UID MOVE completed\r\n")]
            } else {
                vec![format!("{tag} OK completed\r\n")]
            }
        })
        .await;

        let db = Db::open_in_memory().unwrap();
        let account = test_mock_account(server.port);
        let account_id = accounts::create(
            &db,
            &crate::models::NewAccount {
                name: account.name.clone(),
                email_address: account.email_address.clone(),
                from_name: account.from_name.clone(),
                imap_host: account.imap_host.clone(),
                imap_port: account.imap_port,
                imap_security: account.imap_security.clone(),
                imap_username: account.imap_username.clone(),
                smtp_host: account.smtp_host.clone(),
                smtp_port: account.smtp_port,
                smtp_security: account.smtp_security.clone(),
                smtp_username: account.smtp_username.clone(),
                auth_vault_key: account.auth_vault_key.clone(),
                check_interval_secs: account.check_interval_secs,
            },
        )
        .unwrap();

        let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        let _trash_id = folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();

        let mut sync = ImapSync::new(&account);
        sync.connect("secret").await.unwrap();

        sync.move_uids_to(&db, inbox_id, &[55, 56], "Trash").await.unwrap();

        let cmds = server.received.lock().await.clone();
        let store_idx = cmds.iter().position(|c| c.contains("STORE") && c.contains("\\Seen"));
        let move_idx = cmds.iter().position(|c| c.contains("UID MOVE"));

        assert!(store_idx.is_some(), "UID STORE \\Seen was not called when moving to Trash");
        assert!(move_idx.is_some(), "UID MOVE was not called");
        assert!(
            store_idx.unwrap() < move_idx.unwrap(),
            "UID STORE \\Seen must occur BEFORE UID MOVE"
        );
    }
}
