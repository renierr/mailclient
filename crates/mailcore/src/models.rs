//! Domain models. These mirror the SQLite tables 1:1 (see `db/schema.sql`).
//!
//! Times are UTC RFC3339 strings (`2026-09-07T12:00:00+00:00`).
//! Address lists are stored as JSON arrays of strings in SQLite and as
//! `Vec<String>` here.

use serde::{Deserialize, Serialize};

/// Well-known folder roles. Anything else is `Custom`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
    Custom,
}

impl FolderRole {
    /// Canonical lowercase string used in the DB.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Trash => "trash",
            Self::Junk => "junk",
            Self::Archive => "archive",
            Self::Custom => "custom",
        }
    }

    /// Parse a role string; unknown values become `Custom`.
    #[must_use]
    pub fn parse_role(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "inbox" => Self::Inbox,
            "sent" => Self::Sent,
            "drafts" => Self::Drafts,
            "trash" => Self::Trash,
            "junk" | "spam" => Self::Junk,
            "archive" => Self::Archive,
            _ => Self::Custom,
        }
    }
}

impl std::str::FromStr for FolderRole {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_role(s))
    }
}

/// One mail account (IMAP + SMTP config). No passwords — see `auth_vault_key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub email_address: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: String,
    pub imap_username: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub smtp_username: String,
    /// Key into the OS keyring; never a secret itself.
    pub auth_vault_key: String,
    pub check_interval_secs: u64,
    pub created_at: String,
    pub updated_at: String,
}

/// Fields needed to create an account.
#[derive(Debug, Clone)]
pub struct NewAccount {
    pub name: String,
    pub email_address: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: String,
    pub imap_username: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub smtp_username: String,
    pub auth_vault_key: String,
    pub check_interval_secs: u64,
}

/// One IMAP folder of an account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: i64,
    pub account_id: i64,
    pub path: String,
    pub delimiter: String,
    pub role: FolderRole,
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub subscribed: bool,
    pub last_sync_at: Option<String>,
}

/// One cached message (headers + bodies).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub account_id: i64,
    pub folder_id: i64,
    pub uid: u32,
    pub message_id_header: Option<String>,
    pub thread_id: Option<String>,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub to_addrs: Vec<String>,
    pub cc_addrs: Vec<String>,
    pub bcc_addrs: Vec<String>,
    pub reply_to: Option<String>,
    pub date: Option<String>,
    pub snippet: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_draft: bool,
    pub has_attachments: bool,
    pub keywords: Vec<String>,
    pub size: u64,
    pub downloaded_full: bool,
}

/// Fields needed to insert a message (UID-identified).
#[derive(Debug, Clone)]
pub struct NewMessage {
    pub account_id: i64,
    pub folder_id: i64,
    pub uid: u32,
    pub message_id_header: Option<String>,
    pub thread_id: Option<String>,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub to_addrs: Vec<String>,
    pub cc_addrs: Vec<String>,
    pub bcc_addrs: Vec<String>,
    pub reply_to: Option<String>,
    pub date: Option<String>,
    pub snippet: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub is_draft: bool,
    pub has_attachments: bool,
    pub keywords: Vec<String>,
    pub size: u64,
    pub downloaded_full: bool,
}

/// Attachment with bytes stored in SQLite (`data` BLOB). `storage_path` is a
/// legacy disk pointer (old rows only, never written by new code).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: i64,
    pub message_id: i64,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub size: u64,
    pub content_id: Option<String>,
    pub storage_path: Option<String>,
    /// Raw bytes. Skipped in JSON feeds — fetch via `get_attachment` / save
    /// path instead of serializing blobs into the QML feed.
    #[serde(skip_serializing, default)]
    pub data: Option<Vec<u8>>,
    pub is_inline: bool,
}

/// Fields needed to store one attachment. `data=None` is metadata only
/// (names/sizes synced, bytes fetched on explicit user request);
/// `Some(bytes)` is the full row after an on-demand download.
#[derive(Debug, Clone)]
pub struct NewAttachment {
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub content_id: Option<String>,
    pub size: u64,
    pub data: Option<Vec<u8>>,
    pub is_inline: bool,
}

/// Known contact for autocomplete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub address: String,
    pub name: Option<String>,
    pub times_seen: u64,
    pub last_seen_at: String,
}

/// Outbox entry status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueStatus {
    Queued,
    Sending,
    Sent,
    Failed,
}

impl QueueStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn parse_status(s: &str) -> Self {
        match s {
            "sending" => Self::Sending,
            "sent" => Self::Sent,
            "failed" => Self::Failed,
            _ => Self::Queued,
        }
    }
}

impl std::str::FromStr for QueueStatus {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse_status(s))
    }
}

/// One outbox row.
#[derive(Debug, Clone)]
pub struct QueuedSend {
    pub id: i64,
    pub account_id: i64,
    pub message_id: Option<i64>,
    pub status: QueueStatus,
    pub last_error: Option<String>,
    pub retries: u64,
    pub created_at: String,
    pub updated_at: String,
}
