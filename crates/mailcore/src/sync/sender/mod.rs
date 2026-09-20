//! Outbound sending via SMTP.
//!
//! Safety rule: automated sends (harness, queue workers, tests) may ONLY go
//! to allowlisted recipients — see [`SendPolicy::from_env`] (unset/empty
//! allowlist denies everything). An interactive Send click in the composer is
//! explicit user consent and uses `SendPolicy::Unrestricted`.
//!
//! Passwords arrive as function args (from the OS keyring or test env),
//! never from SQLite.
//!
//! Layout: `policy` (send policy + format), `message` (request + MIME
//! assembly, pure), `addresses` (recipient parsing), `attachments`
//! (outgoing files), `client` ([`SmtpSender`] submission + queue).

mod addresses;
mod attachments;
mod client;
mod message;
mod policy;

#[cfg(test)]
pub(crate) mod support;

pub use addresses::{
    parse_reply_to, sender_domain_is_aligned, strict_mailboxes, to_group_name, valid_mailboxes,
};
pub use attachments::{
    guess_mime, load_outgoing_attachments, MAX_SEND_ATTACHMENT_BYTES, MAX_SEND_ATTACHMENT_COUNT,
};
pub use client::{endpoint_for, SmtpEndpoint, SmtpSender};
pub use message::{format_draft, resolve_bodies, SendRequest};
pub use policy::{effective_format, SendFormat, SendPolicy};
