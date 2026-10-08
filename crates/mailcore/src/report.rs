//! Delivery status notifications (bounces and delivery confirmations, RFC
//! 3464) and read receipts (MDN, RFC 8098) for the reader's report card,
//! and the sent original a report is about.
//!
//! A bounce is a `multipart/report` whose `message/delivery-status` part
//! holds one block of per-message fields and one block per recipient
//! (`Final-Recipient`, `Action`, `Status`, `Diagnostic-Code`). Next to it a
//! `text/rfc822-headers` (or a whole `message/rfc822`) part carries the
//! original's headers, whose Message-ID finds the copy in Sent so the user
//! can edit and resend it. Enhanced status codes (RFC 3463) are explained
//! in plain words; the server's own text stays alongside.
//!
//! A read receipt has a `message/disposition-notification` part instead:
//! who it is from (`Final-Recipient`), what happened to the mail
//! (`Disposition`: displayed, deleted, …) and the `Original-Message-ID`.
//! Only receipts that arrive are read here; the app never sends one.

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
    /// `positive`, `negative`, `warning` or `neutral`: how to colour it.
    pub tone: String,
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
    /// `delivery` (a DSN) or `read` (a read receipt).
    pub kind: String,
    /// `failed`, `delayed` or `delivered`: the worst action reported. For
    /// a read receipt the disposition: `displayed`, `deleted`, `processed`.
    pub outcome: String,
    /// `positive`, `negative`, `warning` or `neutral`, from the outcome.
    pub tone: String,
    /// `Delivery failed`, `Delivered`, `Read`, `Deleted unread`, …
    pub title: String,
    /// One sentence on what the outcome means for the user.
    pub detail: String,
    pub recipients: Vec<ReportRecipient>,
    /// The server that wrote the report.
    pub reporting_mta: Option<String>,
    /// Subject of the mail the report is about, when its headers came along.
    pub original_subject: Option<String>,
    /// That mail's cached copy (normally in Sent), for "Edit & resend" and
    /// "Open sent mail".
    pub original_folder_id: Option<i64>,
    /// The copy's folder path (Qt selects folders by path).
    pub original_folder_path: Option<String>,
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

/// Whether `mime` is a read receipt's machine-readable part.
pub fn is_disposition_part(mime: Option<&str>) -> bool {
    matches!(
        mime.map(|m| m.trim().to_ascii_lowercase()).as_deref(),
        Some("message/disposition-notification" | "message/global-disposition-notification")
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
            tone: action_tone(&action).to_string(),
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

/// The parsed `message/disposition-notification` body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mdn {
    /// Whose mail app sent the receipt.
    pub recipient: String,
    /// `displayed`, `deleted`, `processed`, `dispatched`, …
    pub disposition: String,
    pub original_message_id: Option<String>,
    pub reporting_ua: Option<String>,
}

/// Parse a `message/disposition-notification` body. `None` without a
/// recipient or a disposition.
pub fn parse_mdn(text: &str) -> Option<Mdn> {
    let fields: Vec<(String, String)> = blocks(text).into_iter().flatten().collect();
    let field = |name: &str| {
        fields
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };
    let recipient = field("Final-Recipient")
        .or_else(|| field("Original-Recipient"))
        .map(typed_value)
        .filter(|r| !r.is_empty())?;
    // `manual-action/MDN-sent-manually; displayed/error` → `displayed`.
    let disposition = field("Disposition")?
        .rsplit(';')
        .next()?
        .split(['/', ' '])
        .find(|t| !t.is_empty())?
        .to_ascii_lowercase();
    Some(Mdn {
        recipient,
        disposition,
        original_message_id: field("Original-Message-ID")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        reporting_ua: field("Reporting-UA")
            .map(|v| v.split(';').next().unwrap_or(v).trim().to_string())
            .filter(|v| !v.is_empty()),
    })
}

/// A disposition as `(label, tone, title, detail)` for the card.
fn disposition_text(disposition: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match disposition {
        "displayed" => (
            "Read",
            "positive",
            "Read",
            "The recipient's mail app reports that your message was opened.",
        ),
        "deleted" => (
            "Deleted unread",
            "neutral",
            "Deleted unread",
            "The recipient's mail app reports that your message was deleted without being opened.",
        ),
        "processed" | "dispatched" => (
            "Received",
            "neutral",
            "Received",
            "The recipient's mail app received your message; that does not mean it was opened.",
        ),
        _ => (
            "Receipt declined",
            "neutral",
            "Receipt declined",
            "The recipient's mail app answered without saying whether the message was opened.",
        ),
    }
}

fn action_tone(action: &str) -> &'static str {
    match action {
        "failed" => "negative",
        "delayed" => "warning",
        "delivered" | "relayed" | "expanded" => "positive",
        _ => "neutral",
    }
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
/// anything but a delivery report or a read receipt.
pub fn delivery_report(
    db: &Db,
    message: &Message,
    listed: &[Attachment],
) -> Option<DeliveryReport> {
    let mdn_part = listed
        .iter()
        .find(|a| is_disposition_part(a.mime_type.as_deref()));
    let status = match mdn_part {
        Some(part) => part,
        None => listed
            .iter()
            .find(|a| is_status_part(a.mime_type.as_deref()))?,
    };
    let mut covered = vec![status.id];
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
    let text = cached_text(db, status.id);
    if mdn_part.is_some() {
        return Some(read_receipt(
            db,
            message,
            text.as_deref().and_then(parse_mdn),
            original_headers,
            covered,
        ));
    }

    let Some(dsn) = text.as_deref().and_then(parse_dsn) else {
        return Some(DeliveryReport {
            kind: "delivery".to_string(),
            outcome: "failed".to_string(),
            tone: "neutral".to_string(),
            title: "Delivery report".to_string(),
            detail: "The report is not downloaded yet.".to_string(),
            recipients: Vec::new(),
            reporting_mta: None,
            original_subject: original_headers.and_then(|h| h.subject),
            original_folder_id: None,
            original_folder_path: None,
            original_uid: None,
            can_resend: false,
            loaded: false,
            covered,
        });
    };

    let original = original_headers
        .as_ref()
        .and_then(|h| h.message_id.as_deref())
        .and_then(|mid| find_original(db, message, mid));
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
        _ => (
            "Delivered",
            "The receiving server put the message into the mailbox. That does not mean it was read.",
        ),
    };
    let mut report = DeliveryReport {
        kind: "delivery".to_string(),
        outcome: outcome.to_string(),
        tone: action_tone(outcome).to_string(),
        title: title.to_string(),
        detail: detail.to_string(),
        can_resend: outcome == "failed" && original.is_some(),
        original_subject: original_headers.and_then(|h| h.subject),
        original_folder_id: None,
        original_folder_path: None,
        original_uid: None,
        recipients: dsn.recipients,
        reporting_mta: dsn.reporting_mta,
        loaded: true,
        covered,
    };
    report.set_original(db, original);
    Some(report)
}

/// The card for a read receipt; `mdn` is `None` while its part is not
/// cached.
fn read_receipt(
    db: &Db,
    message: &Message,
    mdn: Option<Mdn>,
    original_headers: Option<OriginalHeaders>,
    covered: Vec<i64>,
) -> DeliveryReport {
    let original_subject = original_headers.as_ref().and_then(|h| h.subject.clone());
    let Some(mdn) = mdn else {
        return DeliveryReport {
            kind: "read".to_string(),
            outcome: String::new(),
            tone: "neutral".to_string(),
            title: "Read receipt".to_string(),
            detail: "The receipt is not downloaded yet.".to_string(),
            recipients: Vec::new(),
            reporting_mta: None,
            original_subject,
            original_folder_id: None,
            original_folder_path: None,
            original_uid: None,
            can_resend: false,
            loaded: false,
            covered,
        };
    };
    let original = mdn
        .original_message_id
        .as_deref()
        .or_else(|| {
            original_headers
                .as_ref()
                .and_then(|h| h.message_id.as_deref())
        })
        .and_then(|mid| find_original(db, message, mid));
    let (label, tone, title, detail) = disposition_text(&mdn.disposition);
    let mut report = DeliveryReport {
        kind: "read".to_string(),
        tone: tone.to_string(),
        title: title.to_string(),
        detail: detail.to_string(),
        recipients: vec![ReportRecipient {
            address: mdn.recipient,
            action: mdn.disposition.clone(),
            action_label: label.to_string(),
            tone: tone.to_string(),
            status: None,
            reason: None,
            diagnostic: None,
        }],
        outcome: mdn.disposition,
        reporting_mta: mdn.reporting_ua,
        original_subject,
        original_folder_id: None,
        original_folder_path: None,
        original_uid: None,
        can_resend: false,
        loaded: true,
        covered,
    };
    report.set_original(db, original);
    report
}

impl DeliveryReport {
    /// Point the card at the cached original; its subject fills in when
    /// the report carried none.
    fn set_original(&mut self, db: &Db, original: Option<Message>) {
        let Some(o) = original else {
            return;
        };
        if self.original_subject.is_none() {
            self.original_subject = o.subject.clone();
        }
        self.original_folder_path = crate::store::folders::get(db, o.folder_id)
            .ok()
            .map(|f| f.path);
        self.original_folder_id = Some(o.folder_id);
        self.original_uid = Some(o.uid);
    }
}

/// The mail with Message-ID `mid` in the report's account, other than the
/// report itself.
fn find_original(db: &Db, report: &Message, mid: &str) -> Option<Message> {
    messages::find_by_message_id(db, report.account_id, mid)
        .ok()
        .flatten()
        .filter(|o| o.id != report.id)
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
