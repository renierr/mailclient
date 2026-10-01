//! The composer's backend: sending and drafts, shared by both frontends.
//!
//! A frontend hands over the composer's JSON form and gets a result back;
//! everything that decides *what happens* — validation, the send settings,
//! the exactly-once outbox rules, the Sent copy, draft replacement — lives
//! here so a fix lands in both. What stays in the adapters is only how a job
//! is started and how its result is phrased and shown.

mod drafts;
mod form;
mod reply;
mod send;

pub use drafts::{delete_draft, draft_html, drafts_folder, open_draft, save_draft, DraftSaved};
pub use form::ComposeForm;
pub use reply::{reply_address, ReplyAddress};
pub use send::{deliver, prepare_send, PreparedSend, SendOutcome};

/// Inline images: an editor shows an inserted image as a `data:` URL, which
/// the sender turns into a `cid:` part (see `sync::sender::inline`).
pub use crate::sync::sender::{image_data_url, is_inline_image_file};
