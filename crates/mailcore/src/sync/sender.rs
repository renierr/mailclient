//! Outbound sending via SMTP.
//!
//! Safety rule: automated sends (harness, queue workers, tests) may ONLY go
//! to allowlisted recipients — see [`SendPolicy::from_env`] (unset/empty
//! allowlist denies everything). An interactive Send click in the composer is
//! explicit user consent and uses `SendPolicy::Unrestricted`.
//!
//! Passwords arrive as function args (from the OS keyring or test env),
//! never from SQLite.

use lettre::address::{Address, Envelope};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{Message, SmtpTransport, Transport};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Account, FolderRole};
use crate::store::{contacts, folders, queue, settings};
use crate::sync::imap::ImapSync;
use crate::sync::traits::MailSender;

/// Recipient policy enforced before every send.
#[derive(Debug, Clone)]
pub enum SendPolicy {
    /// Only these (lowercased) addresses may receive mail.
    TestAllowlist(Vec<String>),
    /// No restrictions (production, explicit opt-in).
    Unrestricted,
}

impl SendPolicy {
    /// Build from the environment (see module docs).
    /// Unset/empty allowlist denies every recipient.
    ///
    /// `MAILCLIENT_ALLOW_ANY_RECIPIENT=1` lifts the restriction for automated
    /// sends (test harness only — the interactive composer does not consult
    /// this at all). Never export it globally: it belongs in the local
    /// gitignored `.env`, if anywhere.
    #[must_use]
    pub fn from_env() -> Self {
        if std::env::var("MAILCLIENT_ALLOW_ANY_RECIPIENT").as_deref() == Ok("1") {
            return Self::Unrestricted;
        }
        let raw = std::env::var("MAILCLIENT_TEST_SEND_ALLOWLIST").unwrap_or_default();
        let allow = raw
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        Self::TestAllowlist(allow)
    }

    /// Reject if any recipient is not allowlisted.
    pub fn check(&self, recipients: &[&str]) -> Result<()> {
        match self {
            Self::Unrestricted => Ok(()),
            Self::TestAllowlist(allow) => {
                for r in recipients {
                    if !allow.iter().any(|a| a == &r.to_ascii_lowercase()) {
                        return Err(StoreError::InvalidInput(format!(
                            "refusing to send to {r} (test allowlist: {allow:?})"
                        )));
                    }
                }
                Ok(())
            }
        }
    }
}

/// Outgoing body format (user setting `compose_send_format`, resilient).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendFormat {
    /// Smart default: plain text unless the body carries formatting, in
    /// which case HTML (plus a plain twin when `include_plain` is on).
    Auto,
    /// `text/plain` only — safest, always readable.
    Plain,
    /// `multipart/alternative` plain + html — resilient.
    Multipart,
    /// `text/html` only (plus a plain twin when `include_plain` is on).
    Html,
}

impl SendFormat {
    /// Parse user setting; unknown/empty → `Auto`.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match crate::store::settings::normalize_send_format(raw) {
            "plain" => Self::Plain,
            "multipart" => Self::Multipart,
            "html" => Self::Html,
            _ => Self::Auto,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Plain => "plain",
            Self::Multipart => "multipart",
            Self::Html => "html",
        }
    }
}

/// Resolve the wanted format into a concrete wire shape (never `Auto`).
///
/// - `Plain` / `Multipart` pass through untouched.
/// - `Html` gains a plain twin (`Multipart`) when `include_plain` is on.
/// - `Auto`: plain text when the body has no formatting, otherwise HTML
///   (with a plain twin when `include_plain` is on).
#[must_use]
pub fn effective_format(wanted: SendFormat, needs_html: bool, include_plain: bool) -> SendFormat {
    match wanted {
        SendFormat::Auto if !needs_html => SendFormat::Plain,
        SendFormat::Auto | SendFormat::Html if include_plain => SendFormat::Multipart,
        SendFormat::Auto | SendFormat::Html => SendFormat::Html,
        concrete => concrete,
    }
}

/// Max bytes per outgoing attachment (25 MiB, matches IMAP store cap).
pub const MAX_SEND_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;
/// Max files per outgoing message.
pub const MAX_SEND_ATTACHMENT_COUNT: usize = 20;

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
/// - `Auto` is resolved by the caller ([`effective_format`]); passed through
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

/// Guess a MIME type from a filename extension (std-only, no new deps).
/// Unknown extensions fall back to `application/octet-stream`.
#[must_use]
pub fn guess_mime(filename: &str) -> String {
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "log" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" | "pptx" => "application/vnd.ms-powerpoint",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Read + validate composer attachment paths into `(filename, mime, bytes)`.
/// Rejects missing files, directories, oversized files and over-count sends
/// with a human-readable error for the status bar.
pub fn load_outgoing_attachments(paths: &[String]) -> Result<Vec<(String, String, Vec<u8>)>> {
    if paths.len() > MAX_SEND_ATTACHMENT_COUNT {
        return Err(StoreError::InvalidInput(format!(
            "too many attachments ({} > {})",
            paths.len(),
            MAX_SEND_ATTACHMENT_COUNT
        )));
    }
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let raw = p.trim();
        // QML FileDialog hands `file://` URLs (percent-encoded,
        // `file:///C:/…` on Windows) — accept both URL and plain path.
        let path = crate::paths::file_url_to_path(raw);
        let meta = std::fs::metadata(&path).map_err(|_| {
            StoreError::InvalidInput(format!("cannot read attachment: {}", path.display()))
        })?;
        if !meta.is_file() {
            return Err(StoreError::InvalidInput(format!(
                "not a file: {}",
                path.display()
            )));
        }
        if meta.len() > MAX_SEND_ATTACHMENT_BYTES {
            return Err(StoreError::InvalidInput(format!(
                "{} is too large ({} MB > 25 MB)",
                path.display(),
                meta.len() / (1024 * 1024)
            )));
        }
        let bytes = std::fs::read(&path)?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin")
            .to_string();
        let mime = guess_mime(&filename);
        out.push((filename, mime, bytes));
    }
    Ok(out)
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

/// SMTP submission endpoint derived from an account.
#[derive(Debug, Clone)]
pub struct SmtpEndpoint {
    /// `host:port`.
    pub addr: String,
    /// `true` = implicit TLS (465), `false` = STARTTLS (587).
    pub implicit_tls: bool,
}

/// Derive the endpoint from account settings.
#[must_use]
pub fn endpoint_for(account: &Account) -> SmtpEndpoint {
    SmtpEndpoint {
        addr: format!("{}:{}", account.smtp_host, account.smtp_port),
        implicit_tls: account.smtp_port == 465 || account.smtp_security.eq_ignore_ascii_case("tls"),
    }
}

/// SMTP sender bound to one account's settings.
pub struct SmtpSender {
    endpoint: SmtpEndpoint,
    username: String,
    from: String,
}

impl SmtpSender {
    /// Build from account settings (password supplied per-send).
    #[must_use]
    pub fn new(account: &Account) -> Self {
        Self {
            endpoint: endpoint_for(account),
            username: account.smtp_username.clone(),
            from: account.email_address.clone(),
        }
    }

    fn transport(&self, password: &str) -> Result<SmtpTransport> {
        let (host, port) = {
            let mut parts = self.endpoint.addr.rsplitn(2, ':');
            let port: u16 = parts.next().unwrap_or("465").parse().map_err(|_| {
                StoreError::InvalidInput(format!("bad smtp addr {}", self.endpoint.addr))
            })?;
            (parts.next().unwrap_or("").to_string(), port)
        };
        let tls_params = TlsParameters::new(host.clone())
            .map_err(|e| StoreError::InvalidInput(format!("tls setup failed: {e}")))?;
        let mut builder = SmtpTransport::relay(&host)?;
        // EHLO with the sender domain instead of the bare machine hostname:
        // a dotless `EHLO omarchy` trips HELO-based spam heuristics, while
        // the (unavoidable) client IP is logged by the server either way.
        if let Some(domain) = self.from.rsplit('@').next().filter(|d| d.contains('.')) {
            builder = builder.hello_name(ClientId::Domain(domain.to_string()));
        }
        builder = builder
            .port(port)
            .credentials(Credentials::new(
                self.username.clone(),
                password.to_string(),
            ))
            .tls(if self.endpoint.implicit_tls {
                Tls::Wrapper(tls_params)
            } else {
                Tls::Required(tls_params)
            });
        Ok(builder.build())
    }
}

/// Split the To field into real mailboxes: entries that parse become the
/// `To` header, anything else (placeholder text for BCC-only sends) is
/// ignored — the envelope comes from whichever of To/Cc/Bcc parsed.
#[must_use]
pub fn valid_mailboxes(raw: &[String]) -> Vec<lettre::message::Mailbox> {
    raw.iter().filter_map(|s| s.parse().ok()).collect()
}

/// Split a Cc/Bcc field into real mailboxes, rejecting anything that does
/// not parse. Unlike To (which tolerates placeholder text for BCC-only
/// sends), a mistyped Cc/Bcc must fail loudly — silently dropping it would
/// lie about delivery. Display names (`Bob <bob@example.com>`) are fine;
/// the envelope later uses the bare address.
pub fn strict_mailboxes(field: &str, raw: &[String]) -> Result<Vec<lettre::message::Mailbox>> {
    raw.iter()
        .map(|s| {
            s.parse()
                .map_err(|_| StoreError::InvalidInput(format!("invalid address in {field}: {s}")))
        })
        .collect()
}

/// Parse the composer's optional Reply-To into a single mailbox: empty =
/// no header (`None`), anything unparseable is a user-facing error (fail
/// here, not as a silent missing header after send).
pub fn parse_reply_to(raw: &str) -> Result<Option<lettre::message::Mailbox>> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(None);
    }
    t.parse::<lettre::message::Mailbox>()
        .map(Some)
        .map_err(|_| StoreError::InvalidInput(format!("invalid Reply-To address: {t}")))
}

/// Whether the visible sender stays within the configured account domain.
#[must_use]
pub fn sender_domain_is_aligned(from: &str, account_email: &str) -> bool {
    let Some((_, from_domain)) = from.rsplit_once('@') else {
        return false;
    };
    let Some((_, account_domain)) = account_email.rsplit_once('@') else {
        return false;
    };
    !from_domain.is_empty()
        && !account_domain.is_empty()
        && from_domain.eq_ignore_ascii_case(account_domain)
}

/// Group display name for a BCC-only `To:` header (`Friends:;`): RFC 5322
/// `display-name` without specials that would break parsing, ASCII only.
/// Anything else (blank, punctuation-heavy, non-ASCII) falls back to the
/// standard `undisclosed-recipients` group — so recipients always see a
/// proper To line instead of a missing header.
#[must_use]
pub fn to_group_name(text: &str) -> String {
    let t = text.trim();
    let ok = !t.is_empty()
        && t.is_ascii()
        && !t.starts_with(' ')
        && !t.ends_with(' ')
        && !t.contains("  ")
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || "!#$%&'*+-/=?^_`{|}~".contains(c));
    if ok {
        t.to_string()
    } else {
        "undisclosed-recipients".to_string()
    }
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
    for m in to_valid {
        builder = builder.to(m);
    }
    for c in cc {
        builder = builder.cc(c.parse()?);
    }
    for b in bcc {
        builder = builder.bcc(b.parse()?);
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

impl MailSender for SmtpSender {
    fn send_raw(&mut self, db: &Db, account_id: i64, req: &SendRequest<'_>) -> Result<()> {
        let to_boxes = valid_mailboxes(req.to);
        let cc_boxes = strict_mailboxes("Cc", req.cc)?;
        let bcc_boxes = strict_mailboxes("Bcc", req.bcc)?;
        let mut rcpts: Vec<String> = to_boxes.iter().map(|m| m.email.to_string()).collect();
        rcpts.extend(cc_boxes.iter().map(|m| m.email.to_string()));
        rcpts.extend(bcc_boxes.iter().map(|m| m.email.to_string()));
        if rcpts.is_empty() {
            return Err(StoreError::InvalidInput(
                "add at least one recipient (To, Cc or Bcc)".to_string(),
            ));
        }
        let rcpt_refs: Vec<&str> = rcpts.iter().map(String::as_str).collect();
        req.policy.check(&rcpt_refs)?;

        let from_addr: &str = req.from.filter(|s| !s.is_empty()).unwrap_or(&self.from);
        if !from_addr.contains('@') {
            return Err(StoreError::InvalidInput(format!(
                "invalid sender address: {from_addr}"
            )));
        }
        if !self.from.contains('@') {
            return Err(StoreError::InvalidInput(
                "account email address has no domain".to_string(),
            ));
        }
        // SPF, DKIM, and DMARC must align with the visible From domain. SMTP
        // providers sign after submission, so never allow a caller to bypass
        // the composer's same-domain sender restriction.
        if !sender_domain_is_aligned(from_addr, &self.from) {
            return Err(StoreError::InvalidInput(
                "sender domain must match the account domain to preserve SPF/DKIM/DMARC alignment"
                    .to_string(),
            ));
        }
        // Display name from the composer, else the account default — empty
        // means address-only `From:`.
        let from_name = req.from_name.map(str::trim).filter(|s| !s.is_empty());
        let from_box = match from_name {
            Some(name) => lettre::message::Mailbox::new(Some(name.to_string()), from_addr.parse()?),
            None => from_addr.parse()?,
        };
        // Auto resolves per message: formatting present → HTML (with a plain
        // twin when enabled), otherwise plain text.
        let html_src = req
            .body_html
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                if crate::html::looks_like_html(req.body_text) {
                    Some(req.body_text)
                } else {
                    None
                }
            });
        let needs_html = html_src
            .map(|h| crate::html::needs_html_formatting(&crate::html::sanitize_for_send(h)))
            .unwrap_or(false);
        let format = effective_format(req.format, needs_html, req.include_plain);
        let (plain, html) = resolve_bodies(req.body_text, req.body_html, format);
        let files = load_outgoing_attachments(req.attachments)?;
        let to_group = to_boxes
            .is_empty()
            .then(|| to_group_name(&req.to.join(" ")));
        let reply_to = parse_reply_to(req.reply_to.unwrap_or(""))?;
        // Normalized mailbox strings (display names preserved): headers and
        // envelope agree, and malformed strings never reach the SMTP envelope.
        let cc: Vec<String> = cc_boxes.iter().map(|m| m.to_string()).collect();
        let bcc: Vec<String> = bcc_boxes.iter().map(|m| m.to_string()).collect();
        let email = assemble_message(
            from_box,
            req.subject,
            to_boxes,
            to_group.as_deref(),
            &cc,
            &bcc,
            reply_to,
            format,
            plain,
            html,
            &files,
            req.request_mdn,
        )?;
        let raw = email.formatted();
        let queue_id = queue::enqueue_mime(db, account_id, None, &raw, from_addr, &rcpts)?;
        if let Err(e) = self.submit_queued(db, queue_id, req.password) {
            // The user is about to see this failure and owns the retry. Leaving
            // submittable bytes behind would let the next sync deliver the same
            // message again, duplicating whatever they resend by hand.
            let _ = queue::discard_mime(db, queue_id);
            return Err(e);
        }
        if settings::get_bool(db, settings::COLLECT_SENT_CONTACTS).unwrap_or(true) {
            let mut all_rcpts = valid_mailboxes(req.to);
            all_rcpts.extend(valid_mailboxes(req.cc));
            all_rcpts.extend(valid_mailboxes(req.bcc));
            for mb in all_rcpts {
                let addr = mb.email.to_string();
                let name = mb.name.as_deref();
                if let Err(e) = contacts::seen(db, &addr, name) {
                    log::warn!("contacts: could not collect recipient: {e}");
                }
            }
        }
        self.save_sent_copy(db, account_id, req.imap_password, &raw);
        Ok(())
    }
}

impl SmtpSender {
    /// Submit one persisted outbox row. Crash-safe: MIME is already on disk,
    /// `sending` is set before the SMTP round-trip, and the same bytes are
    /// retried after a restart.
    pub fn submit_queued(&self, db: &Db, queue_id: i64, password: &str) -> Result<()> {
        let row = queue::get(db, queue_id)?;
        let raw = row
            .raw_mime
            .as_deref()
            .filter(|b| !b.is_empty())
            .ok_or_else(|| {
                StoreError::InvalidInput(format!("queue entry {queue_id} has no MIME bytes"))
            })?;
        let from = row
            .envelope_from
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| StoreError::InvalidInput("queued send missing envelope from".into()))?;
        if row.envelope_to.is_empty() {
            return Err(StoreError::InvalidInput(
                "queued send has no envelope recipients".into(),
            ));
        }
        queue::mark_sending(db, queue_id)?;
        match self.submit_raw(from, &row.envelope_to, raw, password) {
            Ok(()) => {
                queue::mark_sent(db, queue_id)?;
                Ok(())
            }
            Err(e) => {
                let _ = queue::mark_failed(db, queue_id, &e.to_string());
                Err(e)
            }
        }
    }

    /// Retry every submittable outbox row for this account — rows left in
    /// `sending` by a crash, or earlier flushes that failed transiently. A row
    /// whose failure already reached the user has no MIME left and is skipped.
    ///
    /// Each row is delivered exactly as the original send would have been,
    /// Sent copy included. One row failing does not stop the rest: a permanent
    /// rejection would otherwise block every message queued behind it until
    /// its retry budget ran out. Errors are logged per row; one is returned
    /// only when nothing at all got through, so a partial flush still reports
    /// what it delivered.
    pub fn flush_outbox(
        &self,
        db: &Db,
        account_id: i64,
        password: &str,
        imap_password: Option<&str>,
    ) -> Result<u64> {
        let _ = queue::requeue_interrupted(db, account_id);
        let _ = queue::prune_sent(db);
        let mut sent = 0u64;
        let mut first_error = None;
        for row in queue::list_submittable(db, account_id)? {
            match self.submit_queued(db, row.id, password) {
                Ok(()) => {
                    sent += 1;
                    if settings::get_bool(db, settings::COLLECT_SENT_CONTACTS).unwrap_or(true) {
                        // Envelope only: the display names lived in the
                        // composer form, which is long gone by now.
                        for addr in &row.envelope_to {
                            if let Err(e) = contacts::seen(db, addr, None) {
                                log::warn!("contacts: could not collect recipient: {e}");
                            }
                        }
                    }
                    if let Some(raw) = row.raw_mime.as_deref() {
                        self.save_sent_copy(db, account_id, imap_password, raw);
                    }
                }
                Err(e) => {
                    log::warn!("smtp: outbox entry {} failed: {e}", row.id);
                    first_error.get_or_insert(e);
                }
            }
        }
        match first_error {
            Some(e) if sent == 0 => Err(e),
            _ => Ok(sent),
        }
    }

    fn submit_raw(&self, from: &str, to: &[String], raw: &[u8], password: &str) -> Result<()> {
        let from_addr: Address = from.parse()?;
        let rcpts: Vec<Address> = to
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let envelope = Envelope::new(Some(from_addr), rcpts)
            .map_err(|e| StoreError::InvalidInput(format!("smtp envelope: {e}")))?;
        let response = self.transport(password)?.send_raw(&envelope, raw)?;
        log::info!(
            "smtp: sent to {to:?} via {}: {response:?}",
            self.endpoint.addr
        );
        Ok(())
    }

    /// File the sent MIME bytes into the account's Sent folder (Thunderbird-style).
    /// Best-effort: skipped (with a warning) when the `sent_copy_enabled`
    /// setting is off, no Sent folder is known, or no IMAP credential is
    /// available. Never fails the send itself.
    fn save_sent_copy(&self, db: &Db, account_id: i64, imap_password: Option<&str>, raw: &[u8]) {
        match settings::get_bool(db, settings::SENT_COPY_ENABLED) {
            Ok(true) => {}
            Ok(false) => {
                log::info!("smtp: sent-copy disabled by setting");
                return;
            }
            Err(e) => {
                log::warn!("smtp: cannot read sent-copy setting, skipping copy: {e}");
                return;
            }
        }
        let sent_path = match folders::list_by_account(db, account_id) {
            Ok(list) => list
                .into_iter()
                .find(|f| f.role == FolderRole::Sent)
                .map(|f| f.path),
            Err(e) => {
                log::warn!("smtp: cannot list folders, skipping sent copy: {e}");
                return;
            }
        };
        let Some(sent_path) = sent_path else {
            log::warn!("smtp: no Sent folder known, skipping sent copy");
            return;
        };
        let Some(imap_password) = imap_password else {
            log::warn!("smtp: no IMAP credential, skipping sent copy");
            return;
        };
        let account = match crate::store::accounts::get(db, account_id) {
            Ok(a) => a,
            Err(e) => {
                log::warn!("smtp: cannot load account, skipping sent copy: {e}");
                return;
            }
        };
        let mut imap = ImapSync::new(&account);
        if let Err(e) = imap.connect(imap_password) {
            log::warn!("smtp: IMAP connect failed, skipping sent copy: {e}");
            return;
        }
        if let Err(e) = imap.append_to_folder(&sent_path, raw) {
            log::warn!("smtp: APPEND to {sent_path} failed: {e}");
        } else {
            log::info!("smtp: saved copy to {sent_path}");
        }
        imap.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_account() -> Account {
        Account {
            id: 1,
            name: "n".to_string(),
            email_address: "me@x.y".to_string(),
            from_name: String::new(),
            imap_host: "i".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "u".to_string(),
            smtp_host: "smtp.x".to_string(),
            smtp_port: 587,
            smtp_security: "starttls".to_string(),
            smtp_username: "u".to_string(),
            auth_vault_key: "k".to_string(),
            check_interval_secs: 300,
            created_at: "t".to_string(),
            updated_at: "t".to_string(),
        }
    }

    #[test]
    fn endpoint_prefers_implicit_tls_on_465() {
        let ep = endpoint_for(&test_account());
        assert_eq!(ep.addr, "smtp.x:587");
        assert!(!ep.implicit_tls);
    }

    #[test]
    fn formatting_a_draft_does_not_submit_or_queue_it() {
        let account = test_account();
        let to = vec!["you@example.com".to_string()];
        let cc = Vec::new();
        let bcc = Vec::new();
        let files = Vec::new();
        let request = SendRequest {
            to: &to,
            cc: &cc,
            bcc: &bcc,
            from: None,
            from_name: None,
            reply_to: None,
            subject: "unfinished",
            body_text: "<p>still writing</p>",
            body_html: Some("<p>still writing</p>"),
            attachments: &files,
            format: SendFormat::Auto,
            include_plain: true,
            policy: &SendPolicy::TestAllowlist(Vec::new()),
            password: "",
            imap_password: None,
            request_mdn: false,
        };
        let raw = String::from_utf8(format_draft(&account, &request).unwrap()).unwrap();
        assert!(raw.contains("Subject: unfinished"));
        assert!(raw.contains("still writing"));
    }

    #[test]
    fn policy_blocks_non_allowlisted_recipients() {
        let policy = SendPolicy::TestAllowlist(vec!["allowed@example.com".to_string()]);
        assert!(policy.check(&["allowed@example.com"]).is_ok());
        assert!(policy.check(&["ALLOWED@example.com"]).is_ok());
        assert!(policy.check(&["someone@else.example"]).is_err());
        assert!(policy
            .check(&["allowed@example.com", "evil@example.org"])
            .is_err());
        assert!(SendPolicy::TestAllowlist(vec![])
            .check(&["anyone@example.com"])
            .is_err());
        assert!(SendPolicy::Unrestricted
            .check(&["anyone@example.com"])
            .is_ok());
    }

    #[test]
    fn bodies_resilient_across_formats() {
        // Legacy: composer rich HTML arrived in `body_text`, must not leak tags.
        let (p, h) = resolve_bodies("<b>hi</b><script>x()</script>", None, SendFormat::Plain);
        assert_eq!(p, "hi");
        assert!(h.is_none());
        // Multipart derives the missing plain side.
        let (p2, h2) = resolve_bodies("<p>hi<br>there</p>", None, SendFormat::Multipart);
        assert!(h2.unwrap().contains("hi"));
        assert_eq!(p2, "hi\nthere");
        // Plain input still gains an html twin in multipart mode.
        let (p3, h3) = resolve_bodies("hello", None, SendFormat::Multipart);
        assert_eq!(p3, "hello");
        assert!(h3.unwrap().contains("hello"));
        // Outgoing scripts are stripped even for the sender's own HTML.
        let (_, evil) = resolve_bodies("<p>t</p><script>alert(1)</script>", None, SendFormat::Html);
        assert!(!evil.unwrap().contains("script"));
        // Unknown format string falls back to auto, never panics.
        assert_eq!(SendFormat::parse("nonsense"), SendFormat::Auto);
        assert_eq!(SendFormat::parse(""), SendFormat::Auto);
    }

    #[test]
    fn auto_format_picks_shape_from_content() {
        use SendFormat::{Auto, Html, Multipart, Plain};
        // Plain typing (even wrapped in editor structure) sends text/plain.
        assert_eq!(effective_format(Auto, false, true), Plain);
        assert_eq!(effective_format(Auto, false, false), Plain);
        // Formatting sends multipart by default, html-only on opt-out.
        assert_eq!(effective_format(Auto, true, true), Multipart);
        assert_eq!(effective_format(Auto, true, false), Html);
        // Explicit choices pass through; html gains a twin on opt-in.
        assert_eq!(effective_format(Plain, true, true), Plain);
        assert_eq!(effective_format(Multipart, false, false), Multipart);
        assert_eq!(effective_format(Html, true, true), Multipart);
        assert_eq!(effective_format(Html, true, false), Html);
    }

    #[test]
    fn needs_html_only_for_real_formatting() {
        use crate::html::{needs_html_formatting, sanitize_for_send};
        // Editor structure around plain typing: no HTML needed.
        assert!(!needs_html_formatting(&sanitize_for_send("<p>hello</p>")));
        assert!(!needs_html_formatting(&sanitize_for_send(
            "<div>one</div><div>two<br></div>"
        )));
        assert!(!needs_html_formatting(&sanitize_for_send(
            "plain &amp; simple"
        )));
        // Real formatting needs HTML.
        assert!(needs_html_formatting(&sanitize_for_send(
            "<p>hello <b>bold</b></p>"
        )));
        assert!(needs_html_formatting(&sanitize_for_send(
            "<p>see <a href=\"https://x.example\">this</a></p>"
        )));
        assert!(needs_html_formatting(&sanitize_for_send(
            "<ul><li>one</li></ul>"
        )));
        assert!(needs_html_formatting(&sanitize_for_send(
            "<blockquote>quoted</blockquote>"
        )));
        // A `>`-citation reply draft (what the composer emits for plain
        // mail) carries no formatting: Auto keeps it text/plain.
        assert!(!needs_html_formatting(&sanitize_for_send(
            "<p></p><p>&gt; quoted<br>&gt; more</p>"
        )));
    }

    #[test]
    fn full_html_document_body_survives_sanitizing() {
        // A complete document is the normal case: Qt's rich-text editor emits
        // one, and so does most HTML mail. Listing html/body/meta as
        // content-dropping tags made every such body sanitize to nothing, so
        // the recipient got "(empty)".
        let doc = concat!(
            "<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 4.0//EN\">",
            "<html><head><meta charset=\"utf-8\">",
            "<style type=\"text/css\">p { color: red }</style></head>",
            "<body><p>Hello <b>bold</b> world</p></body></html>"
        );
        for format in [SendFormat::Plain, SendFormat::Multipart, SendFormat::Html] {
            let (plain, html) = resolve_bodies(doc, Some(doc), format);
            assert!(
                plain.contains("Hello") && plain.contains("bold"),
                "plain lost the body for {format:?}: {plain:?}"
            );
            assert_ne!(plain, "(empty)", "for {format:?}");
            if let Some(h) = html {
                assert!(
                    h.contains("Hello"),
                    "html lost the body for {format:?}: {h:?}"
                );
                // Semantic tags survive; the dropped <style> block does not.
                assert!(h.contains("<b>"), "formatting lost for {format:?}: {h:?}");
                assert!(!h.contains("color: red"), "style leaked for {format:?}");
            }
        }
    }

    #[test]
    fn cc_bcc_validation_is_strict() {
        // Bare and display-name forms pass, envelope uses the bare address.
        let ok = strict_mailboxes(
            "Cc",
            &[
                "bob@example.com".to_string(),
                "Bob <bob2@example.com>".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(ok.len(), 2);
        assert_eq!(ok[1].email.to_string(), "bob2@example.com");
        // Garbage fails loudly instead of being dropped or sent raw.
        assert!(strict_mailboxes("Cc", &["bob@".to_string()]).is_err());
        assert!(strict_mailboxes("Bcc", &["".to_string()]).is_err());
    }

    #[test]
    fn reply_to_header_roundtrips() {
        use SendFormat::Plain;
        assert!(parse_reply_to("").unwrap().is_none());
        assert!(parse_reply_to("   ").unwrap().is_none());
        let mbox = parse_reply_to("replies@example.com").unwrap().unwrap();
        assert_eq!(mbox.email.to_string(), "replies@example.com");
        assert!(parse_reply_to("not an address").is_err());

        let from: lettre::message::Mailbox = "me@example.com".parse().unwrap();
        let call = |reply_to| {
            assemble_message(
                from.clone(),
                "hi",
                valid_mailboxes(&["bob@example.com".to_string()]),
                None,
                &[],
                &[],
                reply_to,
                Plain,
                "hello".to_string(),
                None,
                &[],
                false,
            )
            .unwrap()
        };
        let raw = String::from_utf8(call(Some(mbox)).formatted()).unwrap();
        assert!(
            raw.contains("Reply-To: replies@example.com"),
            "no Reply-To: {raw:?}"
        );
        let raw_off = String::from_utf8(call(None).formatted()).unwrap();
        assert!(
            !raw_off.contains("Reply-To"),
            "Reply-To leaked in: {raw_off:?}"
        );
    }

    #[test]
    fn to_field_tolerates_placeholder_text() {
        let s = |x: &str| x.to_string();
        // Real addresses pass through; placeholder text is dropped so a
        // BCC-only send carries no To header.
        let boxes = valid_mailboxes(&[s("bob@example.com"), s("my friends"), s("")]);
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].email.to_string(), "bob@example.com");
        assert!(valid_mailboxes(&[s("anything goes"), s("")]).is_empty());
        assert!(valid_mailboxes(&[]).is_empty());
    }

    #[test]
    fn to_group_name_carries_safe_text() {
        // Plain placeholder text becomes the group display name…
        assert_eq!(to_group_name("my friends"), "my friends");
        assert_eq!(to_group_name("  Family  "), "Family");
        // …anything else falls back to the standard group.
        assert_eq!(to_group_name(""), "undisclosed-recipients");
        assert_eq!(to_group_name("a@b.com, c@d.org"), "undisclosed-recipients");
        assert_eq!(to_group_name("weird; text"), "undisclosed-recipients");
        assert_eq!(to_group_name("Müller"), "undisclosed-recipients");
        assert_eq!(to_group_name("a\r\nBcc: x@y"), "undisclosed-recipients");
    }

    #[test]
    fn bcc_only_send_carries_group_to_and_bcc_envelope() {
        use SendFormat::Plain;
        let from: lettre::message::Mailbox = "me@example.com".parse().unwrap();
        let bcc = vec!["hidden@example.com".to_string()];
        // Placeholder text becomes the group name recipients see…
        let m = assemble_message(
            from.clone(),
            "hi",
            vec![],
            Some("my friends"),
            &[],
            &bcc,
            None,
            Plain,
            "hello".to_string(),
            None,
            &[],
            false,
        )
        .unwrap();
        let raw = String::from_utf8(m.formatted()).unwrap();
        assert!(raw.contains("To: my friends:;"), "no group To: {raw:?}");
        assert_eq!(
            m.envelope().to(),
            &[lettre::Address::new("hidden", "example.com").unwrap()]
        );
        // …blank To falls back to the standard group, envelope intact.
        let m2 = assemble_message(
            from,
            "hi",
            vec![],
            Some("undisclosed-recipients"),
            &[],
            &bcc,
            None,
            Plain,
            "hello".to_string(),
            None,
            &[],
            false,
        )
        .unwrap();
        let raw2 = String::from_utf8(m2.formatted()).unwrap();
        assert!(
            raw2.contains("To: undisclosed-recipients:;"),
            "no fallback To: {raw2:?}"
        );
        assert_eq!(m2.envelope().to().len(), 1);
    }

    #[test]
    fn from_name_renders_display_name() {
        use SendFormat::Plain;
        let named = lettre::message::Mailbox::new(
            Some("John Doe".to_string()),
            "me@example.com".parse().unwrap(),
        );
        let m = assemble_message(
            named,
            "hi",
            valid_mailboxes(&["bob@example.com".to_string()]),
            None,
            &[],
            &[],
            None,
            Plain,
            "hello".to_string(),
            None,
            &[],
            false,
        )
        .unwrap();
        let raw = String::from_utf8(m.formatted()).unwrap();
        assert!(
            raw.contains("From: \"John Doe\" <me@example.com>"),
            "bad From: {raw:?}"
        );
    }

    #[test]
    fn read_receipt_request_adds_mdn_header() {
        use SendFormat::Plain;
        let from: lettre::message::Mailbox = "me@example.com".parse().unwrap();
        let call = |mdn: bool| {
            assemble_message(
                from.clone(),
                "hi",
                valid_mailboxes(&["bob@example.com".to_string()]),
                None,
                &[],
                &[],
                None,
                Plain,
                "hello".to_string(),
                None,
                &[],
                mdn,
            )
            .unwrap()
        };
        let raw = String::from_utf8(call(true).formatted()).unwrap();
        assert!(
            raw.contains("Disposition-Notification-To: me@example.com"),
            "no MDN header: {raw:?}"
        );
        let raw_off = String::from_utf8(call(false).formatted()).unwrap();
        assert!(
            !raw_off.contains("Disposition-Notification-To"),
            "MDN leaked in: {raw_off:?}"
        );
    }

    #[test]
    fn sender_domain_must_match_the_account_domain() {
        assert!(sender_domain_is_aligned(
            "alias@example.com",
            "me@example.com"
        ));
        assert!(sender_domain_is_aligned(
            "alias@EXAMPLE.COM",
            "me@example.com"
        ));
        assert!(!sender_domain_is_aligned(
            "alias@other.example",
            "me@example.com"
        ));
        assert!(!sender_domain_is_aligned("alias", "me@example.com"));
        assert!(!sender_domain_is_aligned("alias@example.com", "me"));
    }

    #[test]
    fn mime_guess_covers_common_types() {
        assert_eq!(guess_mime("a.pdf"), "application/pdf");
        assert_eq!(guess_mime("photo.JPG"), "image/jpeg");
        assert_eq!(guess_mime("notes.txt"), "text/plain");
        assert_eq!(guess_mime("archive.unknownext"), "application/octet-stream");
        assert_eq!(guess_mime("noext"), "application/octet-stream");
    }

    #[test]
    fn outgoing_attachments_read_files_and_reject_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        std::fs::write(&file, b"hi").unwrap();
        // Plain path and file:// URL both work; mime comes from the extension.
        let loaded = load_outgoing_attachments(&[file.to_string_lossy().to_string()]).unwrap();
        assert_eq!(loaded[0].0, "hello.txt");
        assert_eq!(loaded[0].1, "text/plain");
        assert_eq!(loaded[0].2, b"hi");
        let url = format!("file://{}", file.display());
        assert!(load_outgoing_attachments(&[url]).is_ok());
        // Directories and missing files are user-facing errors, not panics.
        assert!(load_outgoing_attachments(&[dir.path().to_string_lossy().to_string()]).is_err());
        assert!(load_outgoing_attachments(&["/does/not/exist.bin".to_string()]).is_err());
    }
}
