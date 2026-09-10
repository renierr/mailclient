//! Outbound sending via SMTP.
//!
//! Safety rule: automated sends (harness, queue workers, tests) may ONLY go
//! to allowlisted recipients — see [`SendPolicy::from_env`] (unset/empty
//! allowlist denies everything). An interactive Send click in the composer is
//! explicit user consent and uses `SendPolicy::Unrestricted`.
//!
//! Passwords arrive as function args (from the OS keyring or test env),
//! never from SQLite.

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{Message, SmtpTransport, Transport};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Account, FolderRole};
use crate::store::{folders, queue, settings};
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
    /// `text/plain` only — safest, always readable.
    Plain,
    /// `multipart/alternative` plain + html — default, resilient.
    Multipart,
    /// `text/html` only.
    Html,
}

impl SendFormat {
    /// Parse user setting; unknown/empty → `Multipart`.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match crate::store::settings::normalize_send_format(raw) {
            "plain" => Self::Plain,
            "html" => Self::Html,
            _ => Self::Multipart,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Multipart => "multipart",
            Self::Html => "html",
        }
    }
}

/// One outbound message (transport details come from the account).
pub struct SendRequest<'a> {
    /// Recipients (checked against the [`SendPolicy`]).
    pub to: &'a [String],
    /// Cc recipients (also policy-checked).
    pub cc: &'a [String],
    /// Sender identity. `None` = account email. Any other address is used
    /// verbatim (server may reject logins that must match the username).
    pub from: Option<&'a str>,
    pub subject: &'a str,
    /// Plain-text source. For composer rich text this may hold HTML source —
    /// [`resolve_bodies`] sorts that out resiliently.
    pub body_text: &'a str,
    /// Optional explicit HTML source (composer rich text). `None` = derive.
    pub body_html: Option<&'a str>,
    /// User-chosen format (see [`SendFormat`]).
    pub format: SendFormat,
    pub policy: &'a SendPolicy,
    /// SMTP password (keyring or test env), never stored.
    pub password: &'a str,
    /// IMAP password for filing the Sent copy (if `sent_copy_enabled`).
    /// `None` skips the copy with a warning; the send still succeeds.
    pub imap_password: Option<&'a str>,
}

/// Split composer input into `(plain, Option<html>)` for the send format.
///
/// - Composer `body_text` holding rich HTML (legacy + current QML sends
///   `TextArea.text` with `RichText`) is detected via
///   [`crate::html::looks_like_html`] and converted, never sent as literal
///   tags in plain mode.
/// - Outgoing HTML is sanitized via [`crate::html::sanitize_for_send`].
/// - Missing sides are derived so `multipart` never has an empty part.
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
        SendFormat::Multipart => {
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

impl MailSender for SmtpSender {
    fn send_raw(&mut self, db: &Db, account_id: i64, req: &SendRequest<'_>) -> Result<()> {
        let mut all: Vec<&str> = req.to.iter().map(String::as_str).collect();
        all.extend(req.cc.iter().map(String::as_str));
        req.policy.check(&all)?;

        let queue_id = queue::enqueue(db, account_id, None)?;
        let from: &str = req.from.filter(|s| !s.is_empty()).unwrap_or(&self.from);
        if !from.contains('@') {
            return Err(StoreError::InvalidInput(format!(
                "invalid sender address: {from}"
            )));
        }
        let mut builder = Message::builder().from(from.parse()?).subject(req.subject);
        for t in req.to {
            builder = builder.to(t.parse()?);
        }
        for c in req.cc {
            builder = builder.cc(c.parse()?);
        }
        let (plain, html) = resolve_bodies(req.body_text, req.body_html, req.format);
        let email = match (req.format, html) {
            (SendFormat::Plain, _) => builder.header(ContentType::TEXT_PLAIN).body(plain)?,
            (_, Some(h)) if req.format == SendFormat::Html => {
                builder.header(ContentType::TEXT_HTML).body(h)?
            }
            (_, Some(h)) => {
                builder.multipart(lettre::message::MultiPart::alternative_plain_html(plain, h))?
            }
            (_, None) => builder.header(ContentType::TEXT_PLAIN).body(plain)?,
        };

        match self.transport(req.password)?.send(&email) {
            Ok(response) => {
                log::info!("smtp: sent to {:?}: {response:?}", req.to);
                queue::mark_sent(db, queue_id)?;
                self.save_sent_copy(db, account_id, req, &email.formatted());
                Ok(())
            }
            Err(e) => {
                queue::mark_failed(db, queue_id, &e.to_string())?;
                Err(StoreError::Smtp(e))
            }
        }
    }
}

impl SmtpSender {
    /// File the sent MIME bytes into the account's Sent folder (Thunderbird-style).
    /// Best-effort: skipped (with a warning) when the `sent_copy_enabled`
    /// setting is off, no Sent folder is known, or no IMAP credential is
    /// available. Never fails the send itself.
    fn save_sent_copy(&self, db: &Db, account_id: i64, req: &SendRequest<'_>, raw: &[u8]) {
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
        let Some(imap_password) = req.imap_password else {
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
        // Unknown format string falls back to multipart, never panics.
        assert_eq!(SendFormat::parse("nonsense"), SendFormat::Multipart);
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
}
