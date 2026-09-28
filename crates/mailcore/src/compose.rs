//! The composer's backend: sending and drafts, shared by both frontends.
//!
//! A frontend hands over the composer's JSON form and gets a result back;
//! everything that decides *what happens* — validation, the send settings,
//! the exactly-once outbox rules, the Sent copy, draft replacement — lives
//! here so a fix lands in both. What stays in the adapters is only how a job
//! is started and how its result is phrased and shown.

mod drafts;
mod form;
mod send;

pub use drafts::{delete_draft, drafts_folder, open_draft, save_draft, DraftSaved};
pub use form::ComposeForm;
pub use send::{deliver, prepare_send, PreparedSend, SendOutcome};
