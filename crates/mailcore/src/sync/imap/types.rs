//! Shared IMAP types: tunables, endpoints, command results, outcomes.
//!
//! Dependency-free by design; protocol verbs live in [`super::session`],
//! orchestration in [`super::engine`].

use imap_types::{
    flag::FlagNameAttribute,
    response::{Data, Status, StatusBody},
};

macro_rules! vec1 {
    ($($x:expr),+ $(,)?) => {
        ::imap_types::core::Vec1::try_from(vec![$($x),+]).expect("vec1 cannot be empty")
    };
}
pub(crate) use vec1;
/// How many UIDs per FETCH round-trip.
pub(crate) const FETCH_CHUNK: usize = 100;

/// Timeout for a single IMAP command round-trip. Without this a dead
/// half-open socket blocks the single `mailclient-net` worker thread forever:
/// `stream.next()` would await a server reply that never arrives.
pub(crate) const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Timeout for TCP connect + TLS handshake each.
pub(crate) const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Newest-N window for a full folder sync (matches the list's initial page,
/// `mailapp`'s `DEFAULT_MESSAGE_LIMIT`; mailcore cannot name it from here).
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
    /// [`Self::implicit_tls`]. Neither set means plaintext, which `ImapSync::connect`
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
/// Result of executing an IMAP command.
#[derive(Debug)]
pub(crate) struct CommandResult {
    pub(crate) data: Vec<Data<'static>>,
    pub(crate) untagged_statuses: Vec<StatusBody<'static>>,
    pub(crate) status: Status<'static>,
}

/// Result of selecting a mailbox.
#[derive(Debug, Default, Clone)]
pub struct SelectResult {
    pub exists: u32,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub highest_modseq: Option<u64>,
    /// QRESYNC VANISHED UID ranges (start, end) — kept as ranges so a
    /// `VANISHED 1:100000` response never materializes 100k entries.
    pub vanished: Vec<(u32, u32)>,
}
/// Discovered folder from LIST/LSUB.
#[derive(Debug, Clone)]
pub struct DiscoveredFolder {
    pub name: String,
    pub delimiter: String,
    pub attributes: Vec<FlagNameAttribute<'static>>,
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

/// Outcome of `ImapSync::search_server_into_cache`, so the UI can say so.
#[derive(Debug, Default)]
pub struct ServerSearchReport {
    /// Folders successfully SELECTed + SEARCHed.
    pub folders_searched: usize,
    /// Full bodies fetched into the cache (bounded).
    pub fetched: u64,
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
}
