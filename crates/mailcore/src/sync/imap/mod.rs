//! IMAP sync (Milestone 1 + 5a CONDSTORE/QRESYNC): connect, LIST folders with role mapping,
//! SELECT + UID FETCH into SQLite, flag push, CONDSTORE/QRESYNC delta sync, and expunge handling.
//!
//! Powered by `imap-next` (sans-I/O protocol state machine over Tokio).
//! Transport: implicit TLS (port 993) or STARTTLS (port 143). Plaintext is
//! refused unless the account explicitly opts in (see AGENT.md security rules).
//!
//! Layout: `session` holds the `imap-next` protocol verbs, `engine` the
//! [`ImapSync`] orchestration, `folders` the multi-pass discovery, and
//! `roles` / `seq` / `tls` / `types` / `parse` / `search` the small helpers.
//! The mock server harness (`mock`) and regression tests (`tests`) only
//! compile under `cfg(test)`.

mod engine;
mod folders;
mod parse;
mod roles;
mod search;
mod seq;
mod session;
mod tls;
mod types;
mod utf7;

#[cfg(test)]
pub(crate) mod mock;
#[cfg(test)]
mod tests;

pub use engine::ImapSync;
pub use roles::{attr_text, is_selectable, map_folder_role, normalize_folder_path, role_from_name};
pub use session::ImapSession;
pub(crate) use types::vec1;
pub use types::{
    endpoint_for, ArchiveOutcome, DiscoveredFolder, ImapEndpoint, MoveOutcome, SelectResult,
    ServerSearchReport, TrashOutcome, FULL_SYNC_WINDOW, MAX_ATTACHMENTS_PER_MESSAGE,
    MAX_ATTACHMENT_BYTES, OLDER_BATCH, QUICK_SYNC_WINDOW,
};
pub use utf7::{decode_modified_utf7, encode_modified_utf7, mailbox_for_wire};
