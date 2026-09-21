//! Outbound message description and MIME assembly (pure, no I/O).
//!
//! [`SendRequest`] borrows the composer's fields; [`resolve_bodies`] sorts
//! plain/HTML sources resiliently, [`format_draft`] builds the bytes stored
//! as a server-side draft, and [`assemble_message`] builds the final
//! [`lettre::message::Message`] for SMTP submission.

use lettre::address::Envelope;
use lettre::message::header::ContentType;
use lettre::Message;

use crate::error::{Result, StoreError};
use crate::models::Account;

use super::{
    addresses::{parse_reply_to, strict_mailboxes, to_group_name, valid_mailboxes},
    attachments::load_outgoing_attachments,
    policy::{SendFormat, SendPolicy},
};

/// One outbound message (transport details come from the account).
pub struct SendRequest<'a> {
    /// Recipients (checked against the [`SendPolicy`]).
    pub to: &'a [String],
    /// Cc recipients (strictly parsed, then policy-checked).
    pub cc: &'a [String],
    /// Bcc recipients (strictly parsed, then policy-checked, never in the headers).
    pub bcc: &'a [String],
    /// Sender identity. `None` = account email. Any other address is used
    /// verbatim (server may reject logins that must match the username).
    pub from: Option<&'a str>,
    /// Sender display name for `From:` (`None`/empty = address only).
    /// Defaults to the account's `from_name` when the composer sends none.
    pub from_name: Option<&'a str>,
    /// Reply-To for outgoing mail (composer field, optional, one address).
    /// `None`/empty = no header; replies to our mail go to `From`.
    pub reply_to: Option<&'a str>,
    pub subject: &'a str,
    /// Plain-text source. For composer rich text this may hold HTML source —
    /// [`resolve_bodies`] sorts that out resiliently.
    pub body_text: &'a str,
    /// Optional explicit HTML source (composer rich text). `None` = derive.
    pub body_html: Option<&'a str>,
    /// Local file paths to attach (composer FileDialog output). Filenames
    /// default to the path basename, MIME types are guessed by extension.
    pub attachments: &'a [String],
    /// User-chosen format (see [`SendFormat`]); `Auto` is resolved per
    /// message from the body content.
    pub format: SendFormat,
    /// Attach a plain-text twin next to HTML (`compose_include_plain`).
    pub include_plain: bool,
    pub policy: &'a SendPolicy,
    /// SMTP password (keyring or test env), never stored.
    pub password: &'a str,
    /// IMAP password for filing the Sent copy (if `sent_copy_enabled`).
    /// `None` skips the copy with a warning; the send still succeeds.
    pub imap_password: Option<&'a str>,
    /// Ask for a read receipt (`Disposition-Notification-To`, RFC 3798).
    /// Recipients may ignore it; it only asks.
    pub request_mdn: bool,
}
/// Split composer input into `(plain, Option<html>)` for the send format.
///
/// - Composer `body_text` holding rich HTML (legacy + current QML sends
///   `TextArea.text` with `RichText`) is detected via
///   [`crate::html::looks_like_html`] and converted, never sent as literal
///   tags in plain mode.
/// - Outgoing HTML is sanitized via [`crate::html::sanitize_for_send`].
/// - Missing sides are derived so `multipart` never has an empty part.
/// - `Auto` is resolved by the caller ([`super::policy::effective_format`]); passed through
///   here it behaves like `Multipart` (never panics, never empty).
pub fn resolve_bodies(
    body_text: &str,
    body_html: Option<&str>,
    format: SendFormat,
) -> (String, Option<String>) {
    let explicit_html = body_html
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(crate::html::sanitize_for_send);
    let text_holds_html = crate::html::looks_like_html(body_text);
    match format {
        SendFormat::Plain => {
            let plain = if let Some(h) = explicit_html.as_deref() {
                crate::html::html_to_text(h)
            } else if text_holds_html {
                crate::html::html_to_text(body_text)
            } else {
                body_text.trim().to_string()
            };
            (
                if plain.is_empty() {
                    "(empty)".to_string()
                } else {
                    plain
                },
                None,
            )
        }
        SendFormat::Html => {
            let html = if let Some(h) = explicit_html {
                h
            } else if text_holds_html {
                crate::html::sanitize_for_send(body_text)
            } else {
                crate::html::text_to_html(body_text)
            };
            let html = if html.trim().is_empty() {
                "<p>(empty)</p>".to_string()
            } else {
                html
            };
            (crate::html::html_to_text(&html), Some(html))
        }
        SendFormat::Multipart | SendFormat::Auto => {
            let has_explicit = explicit_html.is_some();
            let html = if let Some(h) = explicit_html {
                h
            } else if text_holds_html {
                crate::html::sanitize_for_send(body_text)
            } else if body_text.trim().is_empty() {
                "<p>(empty)</p>".to_string()
            } else {
                crate::html::text_to_html(body_text)
            };
            let mut plain = if text_holds_html || has_explicit {
                crate::html::html_to_text(&html)
            } else {
                body_text.trim().to_string()
            };
            if plain.trim().is_empty() {
                plain = crate::html::html_to_text(&html);
            }
            if plain.trim().is_empty() {
                plain = "(empty)".to_string();
            }
            (plain, Some(html))
        }
    }
}
/// Build the MIME message stored by IMAP as a draft. This does no SMTP work,
/// recipient-policy check, queueing, or Sent-folder filing.
pub fn format_draft(account: &Account, req: &SendRequest<'_>) -> Result<Vec<u8>> {
    let from_addr = req
        .from
        .filter(|s| !s.is_empty())
        .unwrap_or(&account.email_address);
    let from_name = req.from_name.map(str::trim).filter(|s| !s.is_empty());
    let from = match from_name {
        Some(name) => lettre::message::Mailbox::new(Some(name.to_string()), from_addr.parse()?),
        None => from_addr.parse()?,
    };
    // Drafts always preserve rich text when present, independently of the
    // user's send preference; that preference is applied only on Send.
    let (plain, html) = resolve_bodies(req.body_text, req.body_html, SendFormat::Multipart);
    let files = load_outgoing_attachments(req.attachments)?;
    let to = valid_mailboxes(req.to);
    let to_group = to.is_empty().then(|| to_group_name(&req.to.join(" ")));
    let reply_to = parse_reply_to(req.reply_to.unwrap_or(""))?;
    // Same strict Cc/Bcc validation as a real send: a draft carrying a bad
    // address must fail here, not when the user hits Send.
    let cc: Vec<String> = strict_mailboxes("Cc", req.cc)?
        .iter()
        .map(|m| m.to_string())
        .collect();
    let bcc: Vec<String> = strict_mailboxes("Bcc", req.bcc)?
        .iter()
        .map(|m| m.to_string())
        .collect();
    Ok(assemble_message(
        from,
        req.subject,
        to,
        to_group.as_deref(),
        &cc,
        &bcc,
        reply_to,
        SendFormat::Multipart,
        plain,
        html,
        &files,
        req.request_mdn,
    )?
    .formatted())
}
/// Assemble the final [`Message`] from resolved parts (pure, no I/O):
/// headers (incl. the `To:` group fallback), body shape, attachments.
/// Tested directly — SMTP submission itself needs the network.
#[allow(clippy::too_many_arguments)]
pub(crate) fn assemble_message(
    from: lettre::message::Mailbox,
    subject: &str,
    to_valid: Vec<lettre::message::Mailbox>,
    to_group: Option<&str>,
    cc: &[String],
    bcc: &[String],
    reply_to: Option<lettre::message::Mailbox>,
    format: SendFormat,
    plain: String,
    html: Option<String>,
    files: &[(String, String, Vec<u8>)],
    request_mdn: bool,
) -> Result<Message> {
    let mut builder = Message::builder().from(from.clone()).subject(subject);
    // Composer's Reply-To ("replies to my mail go here"): omitted when the
    // field is blank, so replies default to From.
    if let Some(mbox) = reply_to {
        builder = builder.reply_to(mbox);
    }
    // Read receipt request (RFC 3798): the address receipts go back to is
    // the visible sender. Recipients may ignore it; it only asks.
    if request_mdn {
        let value = from.email.to_string();
        builder = builder.raw_header(
            lettre::message::header::HeaderValue::dangerous_new_pre_encoded(
                lettre::message::header::HeaderName::new_from_ascii_str(
                    "Disposition-Notification-To",
                ),
                value.clone(),
                value,
            ),
        );
    }
    // No valid To address (blank or placeholder text on a BCC-only send):
    // emit the RFC 5322 group (`To: Friends:;`, else the standard `To:
    // undisclosed-recipients:;`) so recipients see a proper To line.
    // Arbitrary text is NOT valid as addresses, but IS as a group name. The
    // envelope still comes from Cc/Bcc (an unparseable To contributes
    // nothing to it — lettre's header lookup skips it).
    if to_valid.is_empty() {
        if let Some(group) = to_group {
            let value = format!("{group}:;");
            builder = builder.raw_header(
                lettre::message::header::HeaderValue::dangerous_new_pre_encoded(
                    lettre::message::header::HeaderName::new_from_ascii_str("To"),
                    value.clone(),
                    value,
                ),
            );
        }
    }
    // No recipients at all: only a stored draft gets here (every send path
    // requires ≥1 recipient before this). lettre refuses to build without an
    // envelope, but a draft is stored, never submitted — and `formatted()`
    // serializes headers + body only, so this envelope never persists.
    // Point it at the sender to satisfy the builder.
    let no_recipients = to_valid.is_empty() && cc.is_empty() && bcc.is_empty();
    for m in to_valid {
        builder = builder.to(m);
    }
    for c in cc {
        builder = builder.cc(c.parse()?);
    }
    for b in bcc {
        builder = builder.bcc(b.parse()?);
    }
    if no_recipients {
        builder = builder.envelope(
            Envelope::new(Some(from.email.clone()), vec![from.email.clone()])
                .map_err(|e| StoreError::InvalidInput(format!("cannot address draft: {e}")))?,
        );
    }
    Ok(if files.is_empty() {
        match (format, html) {
            (SendFormat::Plain, _) => builder.header(ContentType::TEXT_PLAIN).body(plain),
            (_, Some(h)) if format == SendFormat::Html => {
                builder.header(ContentType::TEXT_HTML).body(h)
            }
            (_, Some(h)) => {
                builder.multipart(lettre::message::MultiPart::alternative_plain_html(plain, h))
            }
            (_, None) => builder.header(ContentType::TEXT_PLAIN).body(plain),
        }
    } else {
        // Body first (single part or alternative), then one `SinglePart`
        // per file inside a `multipart/mixed` envelope.
        let body = match (format, html) {
            (SendFormat::Plain, _) => lettre::message::MultiPart::mixed()
                .singlepart(lettre::message::SinglePart::plain(plain)),
            (_, Some(h)) if format == SendFormat::Html => {
                lettre::message::MultiPart::mixed().singlepart(lettre::message::SinglePart::html(h))
            }
            (_, Some(h)) => lettre::message::MultiPart::alternative_plain_html(plain, h),
            (_, None) => lettre::message::MultiPart::mixed()
                .singlepart(lettre::message::SinglePart::plain(plain)),
        };
        let mut mixed = lettre::message::MultiPart::mixed().multipart(body);
        for (filename, mime, bytes) in files {
            let ctype = ContentType::parse(mime).unwrap_or_else(|_| {
                ContentType::parse("application/octet-stream").expect("static mime parses")
            });
            mixed = mixed.singlepart(
                lettre::message::Attachment::new(filename.clone()).body(bytes.clone(), ctype),
            );
        }
        builder.multipart(mixed)
    }?)
}

#[cfg(test)]
mod tests;
