//! Provider traits. UI and queue code depend only on these.

use crate::db::Db;
use crate::error::Result;
use crate::models::{Folder, Message};
use crate::sync::sender::SendRequest;

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
    /// Selectable remote folders become local rows (special-use mapped to
    /// roles, everything else kept as `custom`).
    fn sync_folders(&mut self, db: &Db, account_id: i64) -> Result<Vec<Folder>>;
    /// Fetch new/changed messages of one folder into the DB.
    fn sync_folder(&mut self, db: &Db, folder_id: i64) -> Result<SyncReport>;
    /// Push local flag changes (`\Seen`, `\Flagged`) to the server.
    fn push_flags(&mut self, db: &Db, message: &Message) -> Result<()>;
}

/// Sends mail via the account's submission transport (SMTP first).
///
/// Every send is checked against a [`SendPolicy`]: while testing, only
/// explicitly allowlisted recipients are accepted (see `SendPolicy::from_env`).
pub trait MailSender {
    /// Enqueue + immediately send a plain-text message.
    /// HTML/attachments/drafts land in M2.
    fn send_raw(&mut self, db: &Db, account_id: i64, req: &SendRequest<'_>) -> Result<()>;
}