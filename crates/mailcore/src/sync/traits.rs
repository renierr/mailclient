//! Provider traits. UI and queue code depend only on these.

use crate::error::Result;
use crate::models::{Folder, Message};

/// What a sync run discovered (used by tests and later by the UI).
#[derive(Debug, Default)]
pub struct SyncReport {
    /// New/updated messages stored.
    pub fetched: u64,
    /// Messages removed locally (expunged server-side).
    pub expunged: u64,
    /// Folders created/updated.
    pub folders: u64,
}

/// A protocol that can mirror remote folders/messages into the local DB.
///
/// Implemented for IMAP in Milestone 1; JMAP/POP3 follow without UI changes.
pub trait SyncProvider {
    /// Human name, e.g. `"imap"`.
    fn name(&self) -> &'static str;
    /// Refresh the folder list of `account_id` (IMAP LIST).
    fn sync_folders(&mut self, account_id: i64) -> Result<Vec<Folder>>;
    /// Fetch new/changed messages of one folder into the DB.
    fn sync_folder(&mut self, folder_id: i64) -> Result<SyncReport>;
    /// Push local flag changes (`\Seen`, `\Flagged`) to the server.
    fn push_flags(&mut self, message: &Message) -> Result<()>;
}

/// Sends a queued message via the account's submission transport (SMTP first).
pub trait MailSender {
    /// Send `message_id` (a draft row); returns the server-assigned Message-ID.
    fn send_queued(&mut self, account_id: i64, message_id: Option<i64>) -> Result<String>;
}
