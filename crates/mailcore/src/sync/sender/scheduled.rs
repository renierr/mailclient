//! Scheduled-send MIME work: the `Date:` refresh at delivery and the
//! delivery receipt to the account's own address. Pure byte work, no
//! network; the submit itself stays in `client`.

use lettre::message::{Mailbox, Message};

use crate::error::Result;
use crate::models::QueuedSend;

/// Rewrite a stored MIME message's `Date:` header to now (RFC 5322), for a
/// scheduled send whose bytes were built at composition time: without this
/// the recipient's list would sort it under composition day. Byte surgery on
/// the header block only (before the blank line) — bodies, which may hold
/// non-UTF-8 attachment bytes, are never decoded. A folded `Date:` loses its
/// continuations; a missing one is prepended. Never fails the send:
/// anything unexpected goes out with its original bytes.
pub(super) fn refresh_date_header(raw: &[u8]) -> Vec<u8> {
    let now = chrono::Utc::now()
        .format("%a, %d %b %Y %H:%M:%S %z")
        .to_string();
    let crlf = raw.windows(2).any(|w| w == b"\r\n");
    let nl: &[u8] = if crlf { b"\r\n" } else { b"\n" };
    let head_end = find_slice(raw, b"\r\n\r\n")
        .map(|i| i + 4)
        .or_else(|| find_slice(raw, b"\n\n").map(|i| i + 2))
        .unwrap_or(raw.len());
    let (head, body) = (raw[..head_end].to_vec(), &raw[head_end..]);
    let is_date_line =
        |line: &[u8]| line.len() > 5 && line[4] == b':' && line[..4].eq_ignore_ascii_case(b"date");
    let mut out: Vec<u8> = Vec::with_capacity(raw.len() + 64);
    let mut replaced = false;
    let mut skipping_fold = false;
    // Split keeping endings: iterate on `\n`, re-emit the `\r` when present.
    let mut start = 0;
    for (i, b) in head.iter().enumerate() {
        if *b != b'\n' {
            continue;
        }
        let mut line = &head[start..i];
        let had_cr = line.ends_with(b"\r");
        if had_cr {
            line = &line[..line.len() - 1];
        }
        let ending: &[u8] = if had_cr { b"\r\n" } else { b"\n" };
        if skipping_fold {
            if line.starts_with(b" ") || line.starts_with(b"\t") {
                start = i + 1;
                continue;
            }
            skipping_fold = false;
        }
        if !replaced && is_date_line(line) {
            out.extend_from_slice(format!("Date: {now}").as_bytes());
            out.extend_from_slice(ending);
            replaced = true;
            skipping_fold = true;
        } else {
            out.extend_from_slice(line);
            out.extend_from_slice(ending);
        }
        start = i + 1;
    }
    // A header block without trailing newline (or no blank line at all):
    // handle the tail the same way.
    if start < head.len() {
        let line = &head[start..];
        if skipping_fold && (line.starts_with(b" ") || line.starts_with(b"\t")) {
            // Folded tail of the replaced Date: dropped.
        } else if !replaced && is_date_line(line) {
            out.extend_from_slice(format!("Date: {now}").as_bytes());
            replaced = true;
        } else {
            out.extend_from_slice(line);
        }
    }
    if !replaced {
        let mut with_date = format!("Date: {now}").into_bytes();
        with_date.extend_from_slice(nl);
        with_date.extend_from_slice(&out);
        out = with_date;
    }
    out.extend_from_slice(body);
    out
}

/// First index of `needle` in `haystack`, if present.
fn find_slice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Build the receipt for one delivered scheduled send, addressed to
/// `self_addr` (the account's own address).
///
/// Names the original subject, when it went out and how many recipients it
/// had — never the recipients themselves: the receipt is written to the
/// outbox and filed into Sent, and no recipient list belongs in either.
pub(super) fn self_receipt_mime(self_addr: &str, orig: &QueuedSend) -> Result<Vec<u8>> {
    let raw = orig.raw_mime.as_deref().unwrap_or(&[]);
    let subject =
        crate::outbox::subject_from_mime(raw).unwrap_or_else(|| "(no subject)".to_string());
    // The flush running now is the actual delivery (the row's `send_at` is
    // when it was due). Written for a reader, in UTC: the core has no
    // reliable local zone on every platform, and the receipt's own `Date:`
    // shows local time in any mail client anyway.
    let body = format!(
        "Scheduled send delivered.\n\nSubject: {subject}\nSent: {}\nRecipients: {}\n",
        chrono::Utc::now().format("%a, %d %b %Y %H:%M UTC"),
        orig.envelope_to.len(),
    );
    let email = Message::builder()
        .from(self_addr.parse::<Mailbox>()?)
        .to(self_addr.parse::<Mailbox>()?)
        .subject(format!("Scheduled send delivered: {subject}"))
        .body(body)?;
    Ok(email.formatted().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal delivered row for the receipt tests.
    fn held_send() -> QueuedSend {
        QueuedSend {
            id: 1,
            account_id: 1,
            message_id: None,
            status: crate::models::QueueStatus::Queued,
            last_error: None,
            retries: 0,
            raw_mime: None,
            envelope_from: None,
            envelope_to: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
            request_dsn: false,
            send_at: None,
            notify_self: true,
        }
    }

    #[test]
    fn self_receipt_names_subject_time_and_count_but_no_recipients() {
        let raw = b"From: me@example.com\r\nTo: someone@example.com\r\nCc: other@example.com\r\n\
                    Subject: The subject\r\n\r\nbody";
        let orig = QueuedSend {
            raw_mime: Some(raw.to_vec()),
            envelope_from: Some("me@example.com".to_string()),
            envelope_to: vec![
                "someone@example.com".to_string(),
                "other@example.com".to_string(),
            ],
            ..held_send()
        };
        let out = String::from_utf8(self_receipt_mime("me@example.com", &orig).unwrap()).unwrap();
        let (head, body) = out.split_once("\r\n\r\n").expect("header/body split");
        assert!(head.contains("From: me@example.com"), "{head}");
        assert!(head.contains("To: me@example.com"), "{head}");
        assert!(
            head.contains("Subject: Scheduled send delivered: The subject"),
            "{head}"
        );
        assert!(body.contains("Subject: The subject"), "{body}");
        assert!(body.contains("Recipients: 2"), "{body}");
        assert!(body.contains(" UTC"), "{body}");
        // Never the recipients: the receipt goes into Sent too.
        assert!(!out.contains("someone@example.com"), "{out}");
        assert!(!out.contains("other@example.com"), "{out}");
    }

    #[test]
    fn self_receipt_of_a_row_without_bytes_still_reads_as_a_receipt() {
        let out =
            String::from_utf8(self_receipt_mime("me@example.com", &held_send()).unwrap()).unwrap();
        assert!(
            out.contains("Subject: Scheduled send delivered: (no subject)"),
            "{out}"
        );
        assert!(out.contains("Recipients: 0"), "{out}");
    }

    #[test]
    fn date_refresh_rewrites_only_the_header() {
        // Binary bodies (attachments) survive byte-identical; only Date moves.
        let mut raw =
            b"From: a@example.com\r\nDate: Mon, 01 Jan 2000 00:00:00 +0000\r\nSubject: hi\r\n\r\n"
                .to_vec();
        raw.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x62, 0x6F, 0x64, 0x79]);
        let out = refresh_date_header(&raw);
        assert!(
            !out.windows(9).any(|w| w == b"01 Jan 200"),
            "the stale date is gone"
        );
        assert!(out.starts_with(b"From: a@example.com\r\nDate: "));
        assert!(
            out.ends_with(&[0xFF, 0xFE, 0x00, 0x62, 0x6F, 0x64, 0x79]),
            "body bytes are untouched"
        );
        // Exactly one Date header, still CRLF shaped.
        assert_eq!(
            out.windows(5)
                .filter(|w| w.eq_ignore_ascii_case(b"date:"))
                .count(),
            1
        );
        // Missing Date: prepended, blank line kept.
        let nodate = b"Subject: hi\r\n\r\nbody".to_vec();
        let out = refresh_date_header(&nodate);
        assert!(out.starts_with(b"Date: "));
        assert!(out.windows(4).any(|w| w == b"\r\n\r\n"));
        // A folded Date loses its continuations, nothing else moves.
        let folded = b"Date: Mon, 01 Jan\r\n 2000 +0000\r\nSubject: hi\r\n\r\nx".to_vec();
        let out = refresh_date_header(&folded);
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("2000 +0000"), "folded tail dropped");
        assert!(text.contains("Subject: hi"));
        assert_eq!(
            text.matches("Date:").count(),
            1,
            "still exactly one Date header"
        );
    }
}
