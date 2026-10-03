//! Exporting messages as standard RFC 5322 / RFC 2045 `.eml` files.
//!
//! Original top-level headers (routing, DKIM, Received, …) are kept as
//! stored; the MIME body is rebuilt from the cached bodies and attachments by
//! the same lettre tree the composer sends with
//! ([`crate::sync::sender::mime_body`]), so every part header is encoded and
//! folded by lettre, not by hand. An export never contains placeholder parts:
//! [`prepare`] downloads missing attachment bytes first, and [`assemble_eml`]
//! refuses while any are still missing.

use std::path::PathBuf;

use lettre::message::header::{self, HeaderName, HeaderValue, Headers};
use lettre::message::{Mailbox, Mailboxes};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Attachment, Message};
use crate::paths::{fallback_attachment_name, file_url_to_path, safe_attachment_name};
use crate::store::messages;
use crate::sync::sender::{mime_body, InlinePart, SendFormat};

/// Suggested filename for exporting a message as `.eml` (`{subject}.eml` or
/// `message-{uid}.eml`).
pub fn suggested_eml_name(db: &Db, folder_id: i64, uid: u32) -> String {
    let subject = messages::get_by_uid(db, folder_id, uid)
        .ok()
        .and_then(|m| m.subject);
    safe_eml_filename(subject.as_deref(), uid)
}

/// Safe filename for an `.eml` export. The subject is a whole name, not a
/// path, so separators become `_`; the rest of the filesystem rules are
/// [`safe_attachment_name`]'s. Falls back to `message-{uid}.eml`.
pub fn safe_eml_filename(subject: Option<&str>, uid: u32) -> String {
    let fallback = format!("message-{uid}.eml");
    let stem = subject.unwrap_or("").replace(['/', '\\'], "_");
    let stem = stem.trim().trim_matches([' ', '.', '_']);
    if stem.is_empty() || stem.eq_ignore_ascii_case("(no subject)") {
        return fallback;
    }
    let name = safe_attachment_name(Some(&format!("{stem}.eml")), 0);
    if name == fallback_attachment_name(0) {
        fallback
    } else {
        name
    }
}

/// Download whatever attachment bytes (inline parts included) the message
/// is still missing, so [`assemble_eml`] can build a complete file. A no-op
/// without network access when everything is cached.
pub async fn prepare(db: &Db, folder_id: i64, uid: u32) -> std::result::Result<(), String> {
    let m = messages::get_by_uid(db, folder_id, uid).map_err(|e| e.to_string())?;
    crate::sync::attachments::ensure_cached(db, m.id, true).await?;
    Ok(())
}

/// Export a message to a file path or directory (which receives the
/// suggested name). Creates missing parent directories and returns the
/// resolved path.
pub fn export_eml_to(db: &Db, folder_id: i64, uid: u32, target: &str) -> Result<PathBuf> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Err(StoreError::InvalidInput("choose where to save".into()));
    }
    let mut dest = file_url_to_path(trimmed);
    if dest.is_dir() || trimmed.ends_with('/') || trimmed.ends_with('\\') {
        dest.push(suggested_eml_name(db, folder_id, uid));
    } else if dest.extension().is_none() {
        dest.set_extension("eml");
    }
    let bytes = assemble_eml(db, folder_id, uid)?;
    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, bytes)?;
    Ok(dest)
}

/// Assemble full RFC 5322 MIME message bytes for an existing message. Fails
/// when attachment bytes are not cached (run [`prepare`] first).
pub fn assemble_eml(db: &Db, folder_id: i64, uid: u32) -> Result<Vec<u8>> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let files = messages::list_attachments(db, m.id)?;
    assemble_message_eml(&m, &files)
}

/// Pure assembly of message metadata + attachments into RFC 5322 bytes.
pub fn assemble_message_eml(m: &Message, files: &[Attachment]) -> Result<Vec<u8>> {
    let mut inlines = Vec::new();
    let mut attached = Vec::new();
    let mut missing = 0;
    for a in files {
        let Some(bytes) = attachment_bytes(a)? else {
            missing += 1;
            continue;
        };
        let mime = a
            .mime_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".into());
        let cid = a
            .content_id
            .as_deref()
            .map(|c| c.trim().trim_matches(['<', '>']))
            .filter(|c| !c.is_empty());
        match cid {
            Some(cid) if a.is_inline => inlines.push(InlinePart {
                cid: cid.to_string(),
                mime,
                bytes,
            }),
            _ => attached.push((
                a.filename.clone().unwrap_or_else(|| "attachment".into()),
                mime,
                bytes,
            )),
        }
    }
    if missing > 0 {
        return Err(StoreError::InvalidInput(format!(
            "{missing} attachment(s) are not downloaded yet"
        )));
    }

    let plain = m.body_text.clone().filter(|s| !s.is_empty());
    let html = m.body_html.clone().filter(|s| !s.is_empty());
    let format = match (&plain, &html) {
        (Some(_), Some(_)) => SendFormat::Multipart,
        (None, Some(_)) => SendFormat::Html,
        _ => SendFormat::Plain,
    };
    let body = mime_body(format, plain.unwrap_or_default(), html, &inlines, &attached);

    let mut out = match m.raw_headers.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(raw) => filter_raw_headers(raw),
        None => synthesize_headers(m),
    }
    .into_bytes();
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    out.extend_from_slice(&body.formatted());
    Ok(out)
}

/// The cached bytes of one attachment (inline BLOB or legacy file), `None`
/// when they were never downloaded.
fn attachment_bytes(a: &Attachment) -> Result<Option<Vec<u8>>> {
    if let Some(data) = a.data.as_ref().filter(|d| !d.is_empty()) {
        return Ok(Some(data.clone()));
    }
    match a.storage_path.as_deref() {
        Some(path) => Ok(Some(std::fs::read(path)?)),
        None => Ok(None),
    }
}

/// The stored top-level headers minus the MIME ones the rebuilt body
/// supplies, as CRLF lines. Continuation lines follow their header.
fn filter_raw_headers(raw: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in raw.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with([' ', '\t']) {
            if skipping {
                continue;
            }
        } else if let Some((name, _)) = line.split_once(':') {
            skipping = is_mime_body_header(name.trim());
            if skipping {
                continue;
            }
        } else {
            // Not a header (nor a continuation): drop it rather than emit a
            // line that would end the header block early.
            skipping = true;
            continue;
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out
}

fn is_mime_body_header(name: &str) -> bool {
    [
        "content-type",
        "content-transfer-encoding",
        "content-disposition",
        "content-length",
        "mime-version",
    ]
    .iter()
    .any(|h| name.eq_ignore_ascii_case(h))
}

/// Top-level headers rebuilt from the message row (mail stored before raw
/// headers were kept). lettre encodes and folds every value.
fn synthesize_headers(m: &Message) -> String {
    let mut h = Headers::new();
    if let Some(date) = m.date.as_deref() {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date) {
            set_raw(&mut h, "Date", dt.to_rfc2822());
        }
    }
    if let Some(from) = m.from_addr.as_deref().and_then(|a| a.parse().ok()) {
        let name = m.from_name.clone().filter(|n| !n.trim().is_empty());
        h.set(header::From::from(Mailboxes::from(Mailbox::new(
            name, from,
        ))));
    }
    if let Some(to) = mailboxes(&m.to_addrs) {
        h.set(header::To::from(to));
    }
    if let Some(cc) = mailboxes(&m.cc_addrs) {
        h.set(header::Cc::from(cc));
    }
    if let Some(reply_to) = m
        .reply_to
        .as_deref()
        .and_then(|r| r.parse::<Mailbox>().ok())
    {
        h.set(header::ReplyTo::from(Mailboxes::from(reply_to)));
    }
    if let Some(subject) = m.subject.as_deref() {
        h.set(header::Subject::from(subject.to_string()));
    }
    if let Some(mid) = m.message_id_header.as_deref() {
        let mid = mid.trim().trim_matches(['<', '>']);
        if !mid.is_empty() && mid.is_ascii() && !mid.contains(['\r', '\n', ' ', '<', '>']) {
            set_raw(&mut h, "Message-ID", format!("<{mid}>"));
        }
    }
    h.to_string()
}

fn set_raw(h: &mut Headers, name: &'static str, value: String) {
    h.insert_raw(HeaderValue::new(
        HeaderName::new_from_ascii_str(name),
        value,
    ));
}

/// The addresses that parse, `None` when none do.
fn mailboxes(addrs: &[String]) -> Option<Mailboxes> {
    let parsed: Vec<Mailbox> = addrs.iter().filter_map(|a| a.parse().ok()).collect();
    (!parsed.is_empty()).then(|| parsed.into_iter().collect())
}

#[cfg(test)]
mod tests;
