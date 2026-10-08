//! Mails attached to a mail (`message/rfc822`, `.eml`): forwarded as an
//! attachment, or the original inside a bounce. The reader shows each as a
//! card with its headers, a snippet and the text body to expand, instead of
//! an opaque file.

use serde::Serialize;

use crate::models::Attachment;

/// Characters of body text a card carries; a preview, not an archive.
const MAX_BODY_CHARS: usize = 50_000;

/// What the reader's attached-message card shows.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AttachedMessage {
    /// The subject, `(no subject)` when blank. While not `loaded`, the
    /// attachment's name.
    pub subject: String,
    /// `Name <address>` or the bare address.
    pub from: Option<String>,
    /// All To recipients, comma-joined.
    pub to: Option<String>,
    /// Local date and time (`2026-09-12 13:50`).
    pub date: Option<String>,
    /// `from · date`, the line under the subject; `None` when both are.
    pub byline: Option<String>,
    /// The first words of the body on one line.
    pub snippet: String,
    /// The body as plain text (HTML converted), capped.
    pub body_text: String,
    /// The attached mail's own attachments, by count only.
    pub attachment_count: usize,
    /// False while the attachment's bytes are not downloaded.
    pub loaded: bool,
    pub attachment_id: Option<i64>,
    /// Filesystem-safe name to open or save the `.eml` under.
    pub save_name: Option<String>,
}

impl AttachedMessage {
    fn set_attachment(&mut self, att: &Attachment) {
        self.attachment_id = Some(att.id);
        self.save_name = Some(crate::paths::safe_attachment_name_for_mime(
            att.filename.as_deref(),
            att.mime_type.as_deref(),
            att.id,
        ));
    }

    /// The card for `att`: parsed from `bytes`, or a pending card while
    /// they are not cached. `None` when the bytes are no mail.
    pub fn for_attachment(att: &Attachment, bytes: Option<&[u8]>) -> Option<Self> {
        let mut card = match bytes {
            Some(b) => parse_attached(b)?,
            None => AttachedMessage {
                subject: att
                    .filename
                    .as_deref()
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .unwrap_or("Attached message")
                    .to_string(),
                from: None,
                to: None,
                date: None,
                byline: None,
                snippet: String::new(),
                body_text: String::new(),
                attachment_count: 0,
                loaded: false,
                attachment_id: None,
                save_name: None,
            },
        };
        card.set_attachment(att);
        Some(card)
    }
}

/// Whether an attachment is a mail, by MIME or `.eml` name.
pub fn is_message_attachment(filename: Option<&str>, mime: Option<&str>) -> bool {
    let by_name = filename.is_some_and(|f| f.trim().to_ascii_lowercase().ends_with(".eml"));
    let by_mime = matches!(
        mime.map(|m| m.trim().to_ascii_lowercase()).as_deref(),
        Some("message/rfc822" | "message/global")
    );
    by_name || by_mime
}

/// Parse a whole RFC 5322 message. `None` when it has neither a sender, a
/// subject nor a date — no mail at all.
pub fn parse_attached(bytes: &[u8]) -> Option<AttachedMessage> {
    let parsed = mail_parser::MessageParser::default().parse(bytes)?;
    let from = parsed.from().and_then(|a| a.first()).and_then(|a| {
        let addr = a
            .address
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let name = a.name.as_deref().map(str::trim).filter(|s| !s.is_empty());
        match (name, addr) {
            (Some(n), Some(a)) if n != a => Some(format!("{n} <{a}>")),
            (_, Some(a)) => Some(a.to_string()),
            (Some(n), None) => Some(n.to_string()),
            _ => None,
        }
    });
    let date = parsed
        .date()
        .and_then(|d| chrono::DateTime::from_timestamp(d.to_timestamp(), 0))
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    let subject = parsed
        .subject()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if from.is_none() && subject.is_none() && date.is_none() {
        return None;
    }
    let to = parsed.to().map(|list| {
        list.iter()
            .filter_map(|a| a.address.as_deref())
            .collect::<Vec<_>>()
            .join(", ")
    });
    let text = parsed
        .body_text(0)
        .map(|t| t.into_owned())
        .unwrap_or_default();
    let text = text.trim();
    let body_text: String = text.chars().take(MAX_BODY_CHARS).collect();
    let snippet: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
    let date = date.map(|d| crate::feed::full_local_date(Some(&d)));
    let byline = [from.as_deref(), date.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    Some(AttachedMessage {
        subject: subject.unwrap_or_else(|| "(no subject)".to_string()),
        byline: Some(byline).filter(|b| !b.is_empty()),
        from,
        to: to.filter(|t| !t.is_empty()),
        date,
        snippet,
        body_text,
        attachment_count: parsed.attachment_count(),
        loaded: true,
        attachment_id: None,
        save_name: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAIL: &[u8] = b"From: Jane Doe <jane@example.com>\r\n\
To: bob@example.org, carol@example.org\r\n\
Subject: Quarterly numbers\r\n\
Date: Tue, 6 Oct 2026 14:00:00 +0000\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/mixed; boundary=\"X\"\r\n\r\n\
--X\r\nContent-Type: text/html\r\n\r\n<p>Hello <b>Bob</b>,</p><p>see   attached.</p>\r\n\
--X\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"q3.pdf\"\r\n\r\n%PDF-1.4\r\n\
--X--\r\n";

    #[test]
    fn parses_headers_body_and_attachments() {
        let m = parse_attached(MAIL).expect("mail");
        assert_eq!(m.subject, "Quarterly numbers");
        assert_eq!(m.from.as_deref(), Some("Jane Doe <jane@example.com>"));
        assert_eq!(m.to.as_deref(), Some("bob@example.org, carol@example.org"));
        assert!(m.date.is_some());
        assert!(m
            .byline
            .as_deref()
            .unwrap()
            .starts_with("Jane Doe <jane@example.com> · "));
        assert!(m.body_text.contains("Hello"));
        assert!(m.snippet.starts_with("Hello Bob"));
        assert!(!m.snippet.contains('\n'));
        assert_eq!(m.attachment_count, 1);
        assert!(m.loaded);
    }

    #[test]
    fn rejects_non_mail_and_caps_the_body() {
        assert_eq!(parse_attached(b"just some bytes"), None);
        let mut big = b"From: a@example.com\r\nSubject: Big\r\n\r\n".to_vec();
        big.extend(std::iter::repeat_n(b'x', MAX_BODY_CHARS + 10));
        assert_eq!(
            parse_attached(&big).unwrap().body_text.len(),
            MAX_BODY_CHARS
        );
    }

    #[test]
    fn recognises_message_attachments() {
        assert!(is_message_attachment(None, Some("message/rfc822")));
        assert!(is_message_attachment(
            Some("Fwd.EML"),
            Some("application/octet-stream")
        ));
        assert!(!is_message_attachment(Some("a.txt"), Some("text/plain")));
        assert!(!is_message_attachment(
            None,
            Some("message/delivery-status")
        ));
    }
}
