//! Exporting messages as standard RFC 5322 / RFC 2045 `.eml` files.
//!
//! Preserves original headers (routing, technical headers, DKIM, Received, etc.)
//! while regenerating MIME structural headers (boundary markers, Content-Type,
//! Transfer-Encoding) to match the stored bodies and attachments.

use std::path::PathBuf;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Attachment, Message};
use crate::paths::file_url_to_path;
use crate::store::messages;

/// Suggested filename for exporting a message as `.eml` (`{subject}.eml` or `message-{uid}.eml`).
pub fn suggested_eml_name(db: &Db, folder_id: i64, uid: u32) -> String {
    let subject = messages::get_by_uid(db, folder_id, uid)
        .ok()
        .and_then(|m| m.subject);
    safe_eml_filename(subject.as_deref(), uid)
}

/// Safe filename for an `.eml` export: forbidden filesystem characters are replaced,
/// whitespace and dots trimmed, and falls back to `message-{uid}.eml` if empty.
pub fn safe_eml_filename(subject: Option<&str>, uid: u32) -> String {
    let clean = clean_eml_subject(subject.unwrap_or(""));
    if clean.is_empty() {
        format!("message-{uid}.eml")
    } else {
        format!("{clean}.eml")
    }
}

/// Sanitize subject for use as a filename stem.
pub fn clean_eml_subject(raw: &str) -> String {
    let base = raw.trim();
    if base.is_empty() || base.eq_ignore_ascii_case("(no subject)") {
        return String::new();
    }
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches([' ', '.', '_']);
    if trimmed.is_empty() {
        return String::new();
    }
    let mut cut = trimmed.len();
    if trimmed.chars().count() > 80 {
        cut = trimmed
            .char_indices()
            .nth(80)
            .map(|(i, _)| i)
            .unwrap_or(trimmed.len());
    }
    trimmed[..cut].trim_end_matches([' ', '.', '_']).to_string()
}

/// Export a message to a file path or directory (which receives the suggested name).
/// Creates missing parent directories and returns the resolved path.
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
    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = assemble_eml(db, folder_id, uid)?;
    std::fs::write(&dest, bytes)?;
    Ok(dest)
}

/// Assemble full RFC 5322 MIME message bytes for an existing message.
pub fn assemble_eml(db: &Db, folder_id: i64, uid: u32) -> Result<Vec<u8>> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let files = messages::list_attachments(db, m.id)?;
    assemble_message_eml(&m, &files)
}

/// Pure assembly of message metadata + attachments into RFC 5322 bytes.
pub fn assemble_message_eml(m: &Message, files: &[Attachment]) -> Result<Vec<u8>> {
    let mut out = Vec::new();

    // 1. Headers (filter existing or synthesize if absent)
    let headers = if let Some(raw) = m.raw_headers.as_deref().filter(|s| !s.trim().is_empty()) {
        filter_raw_headers(raw)
    } else {
        synthesize_headers(m)
    };
    out.extend_from_slice(headers.as_bytes());
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");

    let has_attachments = !files.is_empty();
    let has_plain = m.body_text.as_deref().is_some_and(|s| !s.is_empty());
    let has_html = m.body_html.as_deref().is_some_and(|s| !s.is_empty());

    let boundary_mixed = format!(
        "----=_Part_Mixed_{}_{}",
        m.uid,
        uuid::Uuid::new_v4().simple()
    );
    let boundary_alt = format!("----=_Part_Alt_{}_{}", m.uid, uuid::Uuid::new_v4().simple());

    if has_attachments {
        out.extend_from_slice(
            format!("Content-Type: multipart/mixed; boundary=\"{boundary_mixed}\"\r\n\r\n")
                .as_bytes(),
        );

        // Body part within multipart/mixed
        out.extend_from_slice(format!("--{boundary_mixed}\r\n").as_bytes());
        append_body_part(
            &mut out,
            &boundary_alt,
            has_plain,
            has_html,
            m.body_text.as_deref(),
            m.body_html.as_deref(),
        );

        // Attachments
        for a in files {
            out.extend_from_slice(format!("--{boundary_mixed}\r\n").as_bytes());
            let filename = a.filename.as_deref().unwrap_or("attachment");
            let mime = a.mime_type.as_deref().unwrap_or("application/octet-stream");
            if a.is_inline {
                out.extend_from_slice(
                    format!("Content-Type: {mime}; name=\"{filename}\"\r\n").as_bytes(),
                );
                out.extend_from_slice(
                    format!("Content-Disposition: inline; filename=\"{filename}\"\r\n").as_bytes(),
                );
                if let Some(cid) = a.content_id.as_deref().filter(|s| !s.trim().is_empty()) {
                    let clean_cid = cid.trim_matches(['<', '>']);
                    out.extend_from_slice(format!("Content-ID: <{clean_cid}>\r\n").as_bytes());
                }
            } else {
                out.extend_from_slice(
                    format!("Content-Type: {mime}; name=\"{filename}\"\r\n").as_bytes(),
                );
                out.extend_from_slice(
                    format!("Content-Disposition: attachment; filename=\"{filename}\"\r\n")
                        .as_bytes(),
                );
            }

            if let Some(data) = a.data.as_deref() {
                out.extend_from_slice(b"Content-Transfer-Encoding: base64\r\n\r\n");
                out.extend_from_slice(format_base64(data).as_bytes());
            } else {
                out.extend_from_slice(b"Content-Transfer-Encoding: 8bit\r\n\r\n");
                out.extend_from_slice(b"[Attachment data not cached locally]\r\n");
            }
        }

        out.extend_from_slice(format!("--{boundary_mixed}--\r\n").as_bytes());
    } else if has_plain && has_html {
        out.extend_from_slice(
            format!("Content-Type: multipart/alternative; boundary=\"{boundary_alt}\"\r\n\r\n")
                .as_bytes(),
        );
        append_alternative_parts(
            &mut out,
            &boundary_alt,
            m.body_text.as_deref().unwrap_or(""),
            m.body_html.as_deref().unwrap_or(""),
        );
    } else if has_html {
        out.extend_from_slice(
            b"Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
        );
        out.extend_from_slice(to_crlf(m.body_html.as_deref().unwrap_or("")).as_bytes());
        out.extend_from_slice(b"\r\n");
    } else {
        out.extend_from_slice(
            b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
        );
        out.extend_from_slice(to_crlf(m.body_text.as_deref().unwrap_or("")).as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    Ok(out)
}

fn append_body_part(
    out: &mut Vec<u8>,
    boundary_alt: &str,
    has_plain: bool,
    has_html: bool,
    plain: Option<&str>,
    html: Option<&str>,
) {
    if has_plain && has_html {
        out.extend_from_slice(
            format!("Content-Type: multipart/alternative; boundary=\"{boundary_alt}\"\r\n\r\n")
                .as_bytes(),
        );
        append_alternative_parts(out, boundary_alt, plain.unwrap_or(""), html.unwrap_or(""));
    } else if has_html {
        out.extend_from_slice(
            b"Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
        );
        out.extend_from_slice(to_crlf(html.unwrap_or("")).as_bytes());
        out.extend_from_slice(b"\r\n");
    } else {
        out.extend_from_slice(
            b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
        );
        out.extend_from_slice(to_crlf(plain.unwrap_or("")).as_bytes());
        out.extend_from_slice(b"\r\n");
    }
}

fn append_alternative_parts(out: &mut Vec<u8>, boundary_alt: &str, plain: &str, html: &str) {
    out.extend_from_slice(format!("--{boundary_alt}\r\n").as_bytes());
    out.extend_from_slice(
        b"Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
    );
    out.extend_from_slice(to_crlf(plain).as_bytes());
    out.extend_from_slice(b"\r\n");

    out.extend_from_slice(format!("--{boundary_alt}\r\n").as_bytes());
    out.extend_from_slice(
        b"Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
    );
    out.extend_from_slice(to_crlf(html).as_bytes());
    out.extend_from_slice(b"\r\n");

    out.extend_from_slice(format!("--{boundary_alt}--\r\n").as_bytes());
}

/// Base64 with 76-character CRLF line wraps per RFC 2045.
fn format_base64(bytes: &[u8]) -> String {
    let b64 = crate::html::base64_encode(bytes);
    if b64.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(b64.len() + (b64.len() / 76) * 2 + 2);
    for chunk in b64.as_bytes().chunks(76) {
        if let Ok(s) = std::str::from_utf8(chunk) {
            out.push_str(s);
            out.push_str("\r\n");
        }
    }
    out
}

/// Convert all line endings (`\n`, `\r\n`, lone `\r`) to RFC 5322 canonical CRLF (`\r\n`).
fn to_crlf(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 20);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            out.push('\r');
            if chars.peek() == Some(&'\n') {
                out.push(chars.next().unwrap());
            } else {
                out.push('\n');
            }
        } else if c == '\n' {
            out.push('\r');
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

fn filter_raw_headers(raw: &str) -> String {
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in raw.lines() {
        let trimmed_line = line.trim_end_matches(['\r', '\n']);
        if trimmed_line.starts_with(' ') || trimmed_line.starts_with('\t') {
            if !skipping {
                kept.push(trimmed_line);
            }
        } else if let Some((name, _)) = trimmed_line.split_once(':') {
            let name = name.trim();
            if is_mime_body_header(name) {
                skipping = true;
            } else {
                skipping = false;
                kept.push(trimmed_line);
            }
        } else {
            skipping = false;
        }
    }
    let mut out = kept.join("\r\n");
    if !out.is_empty() {
        out.push_str("\r\n");
    }
    out
}

fn is_mime_body_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("content-type")
        || name.eq_ignore_ascii_case("content-transfer-encoding")
        || name.eq_ignore_ascii_case("content-disposition")
        || name.eq_ignore_ascii_case("content-length")
        || name.eq_ignore_ascii_case("mime-version")
}

fn synthesize_headers(m: &Message) -> String {
    let mut out = String::new();
    if let Some(date) = m.date.as_deref() {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(date) {
            out.push_str(&format!("Date: {}\r\n", dt.to_rfc2822()));
        } else {
            out.push_str(&format!("Date: {date}\r\n"));
        }
    }
    if let Some(from) = m.from_addr.as_deref() {
        if let Some(name) = m.from_name.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!("From: \"{name}\" <{from}>\r\n"));
        } else {
            out.push_str(&format!("From: <{from}>\r\n"));
        }
    }
    if !m.to_addrs.is_empty() {
        out.push_str(&format!("To: {}\r\n", m.to_addrs.join(", ")));
    }
    if !m.cc_addrs.is_empty() {
        out.push_str(&format!("Cc: {}\r\n", m.cc_addrs.join(", ")));
    }
    if let Some(subject) = m.subject.as_deref() {
        out.push_str(&format!("Subject: {subject}\r\n"));
    }
    if let Some(mid) = m.message_id_header.as_deref() {
        if mid.starts_with('<') && mid.ends_with('>') {
            out.push_str(&format!("Message-ID: {mid}\r\n"));
        } else {
            out.push_str(&format!("Message-ID: <{mid}>\r\n"));
        }
    }
    if let Some(reply_to) = m.reply_to.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!("Reply-To: {reply_to}\r\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::models::{Attachment, FolderRole, NewAccount, NewMessage};
    use crate::store::{accounts, folders, messages};
    use mail_parser::MimeHeaders;

    fn make_test_db() -> (Db, i64, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc_id = accounts::create(
            &db,
            &NewAccount {
                name: "Work".into(),
                email_address: "me@example.com".into(),
                from_name: "Me".into(),
                imap_host: "imap.example.com".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                imap_username: "me".into(),
                smtp_host: "smtp.example.com".into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                smtp_username: "me".into(),
                auth_vault_key: "k".into(),
                check_interval_secs: 300,
            },
        )
        .unwrap();
        let folder_id = folders::upsert(&db, acc_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        (db, acc_id, folder_id)
    }

    #[test]
    fn filename_sanitization() {
        assert_eq!(
            safe_eml_filename(Some("Project Update / Q3: Review"), 42),
            "Project Update _ Q3_ Review.eml"
        );
        assert_eq!(safe_eml_filename(None, 42), "message-42.eml");
        assert_eq!(
            safe_eml_filename(Some("(no subject)"), 42),
            "message-42.eml"
        );
        assert_eq!(safe_eml_filename(Some("   "), 42), "message-42.eml");
        assert_eq!(safe_eml_filename(Some("..."), 42), "message-42.eml");
    }

    #[test]
    fn assemble_plain_message_parses_with_mail_parser() {
        let msg = Message {
            id: 1,
            account_id: 1,
            folder_id: 1,
            uid: 101,
            message_id_header: Some("msg-101@example.com".into()),
            thread_id: None,
            subject: Some("Plain test".into()),
            from_addr: Some("sender@example.com".into()),
            from_name: Some("Sender Name".into()),
            to_addrs: vec!["recipient@example.com".into()],
            cc_addrs: vec![],
            bcc_addrs: vec![],
            reply_to: None,
            date: Some("2026-10-03T10:00:00Z".into()),
            snippet: Some("Hello plain".into()),
            body_text: Some("Hello plain body\nSecond line".into()),
            body_html: None,
            raw_headers: None,
            is_read: true,
            is_starred: false,
            is_draft: false,
            has_attachments: false,
            keywords: vec![],
            size: 100,
            downloaded_full: true,
        };

        let bytes = assemble_message_eml(&msg, &[]).unwrap();
        let parsed = mail_parser::MessageParser::default().parse(&bytes).unwrap();

        assert_eq!(parsed.subject().unwrap(), "Plain test");
        assert_eq!(
            parsed.from().unwrap().first().unwrap().address().unwrap(),
            "sender@example.com"
        );
        assert!(parsed.body_text(0).unwrap().contains("Hello plain body"));
        assert!(parsed.body_text(0).unwrap().contains("Second line"));
    }

    #[test]
    fn assemble_multipart_with_attachments_parses_cleanly() {
        let msg = Message {
            id: 2,
            account_id: 1,
            folder_id: 1,
            uid: 102,
            message_id_header: Some("msg-102@example.com".into()),
            thread_id: None,
            subject: Some("Invoice & Receipt".into()),
            from_addr: Some("billing@example.com".into()),
            from_name: None,
            to_addrs: vec!["client@example.com".into()],
            cc_addrs: vec!["accounting@example.com".into()],
            bcc_addrs: vec![],
            reply_to: None,
            date: Some("2026-10-03T12:00:00Z".into()),
            snippet: Some("Invoice details".into()),
            body_text: Some("Please find attached.".into()),
            body_html: Some("<p>Please find <b>attached</b>.</p>".into()),
            raw_headers: Some("X-Custom: verified\r\nSubject: Invoice & Receipt\r\nFrom: billing@example.com\r\nContent-Type: old/type\r\n".into()),
            is_read: true,
            is_starred: true,
            is_draft: false,
            has_attachments: true,
            keywords: vec![],
            size: 250,
            downloaded_full: true,
        };

        let attachment = Attachment {
            id: 1,
            message_id: 2,
            filename: Some("invoice.pdf".into()),
            mime_type: Some("application/pdf".into()),
            size: 4,
            content_id: None,
            data: Some(b"%PDF".to_vec()),
            is_inline: false,
            storage_path: None,
        };

        let bytes = assemble_message_eml(&msg, &[attachment]).unwrap();
        let parsed = mail_parser::MessageParser::default().parse(&bytes).unwrap();

        assert_eq!(parsed.subject().unwrap(), "Invoice & Receipt");
        assert_eq!(
            parsed.header("X-Custom").and_then(|h| h.as_text()),
            Some("verified")
        );
        assert!(parsed
            .body_text(0)
            .unwrap()
            .contains("Please find attached."));
        assert!(parsed.body_html(0).unwrap().contains("<b>attached</b>"));

        let attachments: Vec<_> = parsed.attachments().collect();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].attachment_name(), Some("invoice.pdf"));
        assert_eq!(attachments[0].contents(), b"%PDF");
    }

    #[test]
    fn export_eml_to_creates_file_in_dir() {
        let (db, acc_id, folder_id) = make_test_db();
        let msg = NewMessage {
            account_id: acc_id,
            folder_id,
            uid: 103,
            message_id_header: None,
            thread_id: None,
            subject: Some("Report".into()),
            from_addr: Some("boss@example.com".into()),
            from_name: None,
            to_addrs: vec!["team@example.com".into()],
            cc_addrs: vec![],
            bcc_addrs: vec![],
            reply_to: None,
            date: Some("2026-10-03T14:00:00Z".into()),
            snippet: Some("Quarterly".into()),
            body_text: Some("Quarterly results".into()),
            body_html: None,
            raw_headers: None,
            is_read: true,
            is_starred: false,
            is_draft: false,
            has_attachments: false,
            keywords: vec![],
            size: 50,
            downloaded_full: true,
        };
        messages::upsert(&db, &msg).unwrap();

        let temp = tempfile::tempdir().unwrap();
        let out_path = export_eml_to(&db, folder_id, 103, temp.path().to_str().unwrap()).unwrap();
        assert!(out_path.exists());
        assert_eq!(out_path.file_name().unwrap(), "Report.eml");

        let content = std::fs::read(&out_path).unwrap();
        assert!(content.starts_with(b"Date:"));
        assert!(String::from_utf8_lossy(&content).contains("Quarterly results"));
    }
}
