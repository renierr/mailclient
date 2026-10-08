//! Delivery status notifications (bounces, RFC 3464) for the reader's
//! delivery report card, and the sent original a bounce reports on.
//!
//! A bounce is a `multipart/report` whose `message/delivery-status` part
//! holds one block of per-message fields and one block per recipient
//! (`Final-Recipient`, `Action`, `Status`, `Diagnostic-Code`). Next to it a
//! `text/rfc822-headers` (or a whole `message/rfc822`) part carries the
//! original's headers, whose Message-ID finds the copy in Sent so the user
//! can edit and resend it. Enhanced status codes (RFC 3463) are explained
//! in plain words; the server's own text stays alongside.

use serde::Serialize;

use crate::db::Db;
use crate::models::{Attachment, Message};
use crate::store::messages;

/// One recipient the report speaks about.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReportRecipient {
    pub address: String,
    /// `failed`, `delayed`, `delivered`, `relayed` or `expanded`.
    pub action: String,
    /// `action` for display: `Failed`, `Delayed`, `Delivered`, …
    pub action_label: String,
    /// Enhanced status code (`5.1.1`), from `Status` or the diagnostic.
    pub status: Option<String>,
    /// The status code in plain words with the code
    /// (`The address does not exist (5.1.1)`).
    pub reason: Option<String>,
    /// The receiving server's own words (`550 5.1.1 no such user`).
    pub diagnostic: Option<String>,
}

/// What the reader's delivery report card shows.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeliveryReport {
    /// `failed`, `delayed` or `delivered`: the worst action reported.
    pub outcome: String,
    /// `Delivery failed`, `Delivery delayed` or `Delivered`.
    pub title: String,
    /// One sentence on what the outcome means for the user.
    pub detail: String,
    pub recipients: Vec<ReportRecipient>,
    /// The server that wrote the report.
    pub reporting_mta: Option<String>,
    /// Subject of the mail the report is about, when its headers came along.
    pub original_subject: Option<String>,
    /// That mail's cached copy (normally in Sent), for "Edit & resend".
    pub original_folder_id: Option<i64>,
    pub original_uid: Option<u32>,
    /// A failed delivery whose original is cached: the card offers resend.
    pub can_resend: bool,
    /// False while the status part's bytes are not downloaded: only
    /// `title` and `detail` are meaningful then.
    pub loaded: bool,
    /// Attachments the card stands in for (the status and headers parts);
    /// the feed marks them `in_card`.
    #[serde(skip)]
    pub covered: Vec<i64>,
}

/// The parsed `message/delivery-status` body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dsn {
    pub reporting_mta: Option<String>,
    pub recipients: Vec<ReportRecipient>,
}

/// A bounce's sent original and the addresses delivery to failed for.
#[derive(Debug, Clone)]
pub struct Bounce {
    pub original: Message,
    pub failed: Vec<String>,
}

/// Whether `mime` is a delivery status part (plain or internationalised).
pub fn is_status_part(mime: Option<&str>) -> bool {
    matches!(
        mime.map(|m| m.trim().to_ascii_lowercase()).as_deref(),
        Some("message/delivery-status" | "message/global-delivery-status")
    )
}

/// Whether `mime` is a part holding only the original's headers.
pub fn is_headers_part(mime: Option<&str>) -> bool {
    matches!(
        mime.map(|m| m.trim().to_ascii_lowercase()).as_deref(),
        Some("text/rfc822-headers" | "message/global-headers")
    )
}

/// Parse a `message/delivery-status` body. `None` when it names no
/// recipient.
pub fn parse_dsn(text: &str) -> Option<Dsn> {
    let mut dsn = Dsn::default();
    for block in blocks(text) {
        let field = |name: &str| {
            block
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        let recipient = field("Final-Recipient").or_else(|| field("Original-Recipient"));
        let Some(recipient) = recipient else {
            if dsn.reporting_mta.is_none() {
                dsn.reporting_mta = field("Reporting-MTA")
                    .map(typed_value)
                    .filter(|v| !v.is_empty());
            }
            continue;
        };
        let address = typed_value(recipient);
        if address.is_empty() {
            continue;
        }
        let diagnostic = field("Diagnostic-Code")
            .map(typed_value)
            .filter(|d| !d.is_empty());
        let status = field("Status")
            .and_then(status_code)
            .or_else(|| diagnostic.as_deref().and_then(status_code));
        let action = field("Action")
            .map(|a| a.trim().to_ascii_lowercase())
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "failed".to_string());
        dsn.recipients.push(ReportRecipient {
            address,
            action_label: action_label(&action),
            action,
            reason: status
                .as_deref()
                .and_then(|code| explain_status(code).map(|text| format!("{text} ({code})"))),
            status,
            diagnostic,
        });
    }
    (!dsn.recipients.is_empty()).then_some(dsn)
}

fn action_label(action: &str) -> String {
    match action {
        "failed" => "Failed".to_string(),
        "delayed" => "Delayed".to_string(),
        "delivered" => "Delivered".to_string(),
        "relayed" => "Relayed".to_string(),
        "expanded" => "Expanded".to_string(),
        other => other.to_string(),
    }
}

/// Field blocks separated by blank lines; continuation lines (leading
/// blank) are folded into the field before them.
fn blocks(text: &str) -> Vec<Vec<(String, String)>> {
    let mut out: Vec<Vec<(String, String)>> = Vec::new();
    let mut current: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, v)) = current.last_mut() {
                v.push(' ');
                v.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            current.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// `rfc822; bob@example.net` → `bob@example.net`: drop the type prefix.
fn typed_value(v: &str) -> String {
    let v = match v.split_once(';') {
        Some((kind, rest)) if !kind.trim().is_empty() && !kind.contains(' ') => rest,
        _ => v,
    };
    v.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim()
        .to_string()
}

/// The first `d.ddd.ddd` enhanced status code in `s`.
fn status_code(s: &str) -> Option<String> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .find(|t| {
            let parts: Vec<&str> = t.split('.').collect();
            parts.len() == 3
                && matches!(parts[0], "2" | "4" | "5")
                && parts[1..]
                    .iter()
                    .all(|p| (1..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(str::to_string)
}

/// An enhanced status code (RFC 3463) in plain words: the common detail
/// codes by name, anything else by its subject class.
pub fn explain_status(code: &str) -> Option<&'static str> {
    let mut parts = code.trim().splitn(3, '.');
    let class = parts.next()?;
    let subject = parts.next()?;
    let detail = parts.next()?;
    if class == "2" {
        return Some("Delivered");
    }
    let named = match (subject, detail) {
        ("1", "1") => Some("The address does not exist"),
        ("1", "2") => Some("The recipient's domain does not exist"),
        ("1", "3") => Some("The address is not valid"),
        ("1", "6") => Some("The recipient has moved"),
        ("1", "10") => Some("The domain does not accept mail"),
        ("2", "1") => Some("The mailbox is disabled"),
        ("2", "2") => Some("The mailbox is full"),
        ("2", "3") | ("3", "4") => Some("The message is too large"),
        ("3", "2") => Some("The receiving server is not accepting mail"),
        ("4", "1") => Some("The receiving server did not answer"),
        ("4", "4") => Some("The receiving server could not be found"),
        ("4", "6") => Some("The message was caught in a mail loop"),
        ("4", "7") => Some("Delivery took too long and was given up"),
        ("7", "1") => Some("The receiving server refused the message"),
        ("7", "23") | ("7", "25") | ("7", "26") | ("7", "27") => {
            Some("The sender could not be verified (SPF/DKIM/DMARC)")
        }
        _ => None,
    };
    named.or(match subject {
        "1" => Some("Problem with the address"),
        "2" => Some("Problem with the mailbox"),
        "3" => Some("Problem with the receiving system"),
        "4" => Some("Network or routing problem"),
        "5" => Some("Mail protocol problem"),
        "6" => Some("Problem with the message content"),
        "7" => Some("Refused for security or policy reasons"),
        _ => None,
    })
}

/// The report card for `message`, from its listed attachments: `None` for
/// anything but a delivery report.
pub fn delivery_report(
    db: &Db,
    message: &Message,
    listed: &[Attachment],
) -> Option<DeliveryReport> {
    let status = listed
        .iter()
        .find(|a| is_status_part(a.mime_type.as_deref()))?;
    let mut covered = vec![status.id];
    let dsn = cached_text(db, status.id).and_then(|t| parse_dsn(&t));
    let headers = listed
        .iter()
        .find(|a| is_headers_part(a.mime_type.as_deref()));
    covered.extend(headers.map(|a| a.id));
    let original_headers = headers
        .or_else(|| {
            listed.iter().find(|a| {
                crate::attached::is_message_attachment(
                    a.filename.as_deref(),
                    a.mime_type.as_deref(),
                )
            })
        })
        .and_then(|a| cached_bytes(db, a.id))
        .and_then(|b| OriginalHeaders::parse(&b));

    let Some(dsn) = dsn else {
        return Some(DeliveryReport {
            outcome: "failed".to_string(),
            title: "Delivery report".to_string(),
            detail: "The report is not downloaded yet.".to_string(),
            recipients: Vec::new(),
            reporting_mta: None,
            original_subject: original_headers.and_then(|h| h.subject),
            original_folder_id: None,
            original_uid: None,
            can_resend: false,
            loaded: false,
            covered,
        });
    };

    let original = original_headers
        .as_ref()
        .and_then(|h| h.message_id.as_deref())
        .and_then(|mid| {
            messages::find_by_message_id(db, message.account_id, mid)
                .ok()
                .flatten()
        })
        .filter(|o| o.id != message.id);
    let outcome = outcome(&dsn.recipients);
    let (title, detail) = match outcome {
        "failed" => (
            "Delivery failed",
            "The receiving side gave up: the message was not delivered to the addresses below.",
        ),
        "delayed" => (
            "Delivery delayed",
            "The server is still trying to deliver it; there is no need to resend yet.",
        ),
        _ => ("Delivered", "The message was delivered."),
    };
    Some(DeliveryReport {
        outcome: outcome.to_string(),
        title: title.to_string(),
        detail: detail.to_string(),
        can_resend: outcome == "failed" && original.is_some(),
        original_subject: original_headers
            .and_then(|h| h.subject)
            .or_else(|| original.as_ref().and_then(|o| o.subject.clone())),
        original_folder_id: original.as_ref().map(|o| o.folder_id),
        original_uid: original.as_ref().map(|o| o.uid),
        recipients: dsn.recipients,
        reporting_mta: dsn.reporting_mta,
        loaded: true,
        covered,
    })
}

/// The bounce at `(folder_id, uid)` with its cached sent original, for an
/// edit-and-resend draft. Errs when it is not a failed delivery report or
/// the original is not cached.
pub fn bounce(db: &Db, folder_id: i64, uid: u32) -> Result<Bounce, String> {
    let m = messages::get_by_uid(db, folder_id, uid)
        .map_err(|_| "this message is no longer available".to_string())?;
    let listed: Vec<Attachment> = messages::list_attachments(db, m.id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|a| !a.is_inline)
        .collect();
    let report = delivery_report(db, &m, &listed)
        .filter(|r| r.can_resend)
        .ok_or_else(|| "the original message of this report is not available".to_string())?;
    let (Some(f), Some(u)) = (report.original_folder_id, report.original_uid) else {
        return Err("the original message of this report is not available".to_string());
    };
    let original = messages::get_by_uid(db, f, u).map_err(|e| e.to_string())?;
    let failed = report
        .recipients
        .into_iter()
        .filter(|r| r.action == "failed")
        .map(|r| r.address)
        .collect();
    Ok(Bounce { original, failed })
}

/// The worst action among the recipients.
fn outcome(recipients: &[ReportRecipient]) -> &'static str {
    if recipients.iter().any(|r| r.action == "failed") {
        "failed"
    } else if recipients.iter().any(|r| r.action == "delayed") {
        "delayed"
    } else {
        "delivered"
    }
}

struct OriginalHeaders {
    message_id: Option<String>,
    subject: Option<String>,
}

impl OriginalHeaders {
    /// Headers of the original, from a headers-only part or a whole message.
    fn parse(bytes: &[u8]) -> Option<Self> {
        let mut raw = bytes.to_vec();
        // A headers-only part may lack the blank line that ends a header
        // block; without it the last header could read as body.
        raw.extend_from_slice(b"\r\n\r\n");
        let parsed = mail_parser::MessageParser::default().parse(&raw)?;
        let h = OriginalHeaders {
            message_id: parsed.message_id().map(str::to_string),
            subject: parsed
                .subject()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        };
        (h.message_id.is_some() || h.subject.is_some()).then_some(h)
    }
}

fn cached_bytes(db: &Db, attachment_id: i64) -> Option<Vec<u8>> {
    messages::get_attachment(db, attachment_id).ok()?.data
}

fn cached_text(db: &Db, attachment_id: i64) -> Option<String> {
    cached_bytes(db, attachment_id).map(|b| String::from_utf8_lossy(&b).into_owned())
}

#[cfg(test)]
mod tests;
