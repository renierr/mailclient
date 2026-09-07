//! Outbound sending via SMTP (Milestone 1: plain-text + safety policy).
//!
//! Safety rule: while testing, mail may ONLY go to allowlisted recipients.
//! [`SendPolicy::from_env`] reads:
//! - `MAILCLIENT_TEST_SEND_ALLOWLIST`: comma-separated allowlist.
//!   Unset/empty means "deny everything" (safest default).
//! - `MAILCLIENT_ALLOW_ANY_RECIPIENT=1`: unlock real sending (production)
//!
//! Passwords arrive as function args (from the OS keyring or test env),
//! never from SQLite.

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{Message, SmtpTransport, Transport};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::Account;
use crate::store::queue;
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

/// One outbound message (transport details come from the account).
pub struct SendRequest<'a> {
    /// Recipients (checked against the [`SendPolicy`]).
    pub to: &'a [String],
    pub subject: &'a str,
    pub body_text: &'a str,
    pub policy: &'a SendPolicy,
    /// SMTP password (keyring or test env), never stored.
    pub password: &'a str,
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
        implicit_tls: account.smtp_port == 465
            || account.smtp_security.eq_ignore_ascii_case("tls"),
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
            .credentials(Credentials::new(self.username.clone(), password.to_string()))
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
        let refs: Vec<&str> = req.to.iter().map(String::as_str).collect();
        req.policy.check(&refs)?;

        let queue_id = queue::enqueue(db, account_id, None)?;
        let mut builder = Message::builder()
            .from(self.from.parse()?)
            .subject(req.subject);
        for t in req.to {
            builder = builder.to(t.parse()?);
        }
        let email = builder
            .header(ContentType::TEXT_PLAIN)
            .body(req.body_text.to_string())?;

        match self.transport(req.password)?.send(&email) {
            Ok(response) => {
                log::info!("smtp: sent to {:?}: {response:?}", req.to);
                queue::mark_sent(db, queue_id)?;
                Ok(())
            }
            Err(e) => {
                queue::mark_failed(db, queue_id, &e.to_string())?;
                Err(StoreError::Smtp(e))
            }
        }
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
        assert!(policy.check(&["allowed@example.com", "evil@example.org"]).is_err());
        assert!(SendPolicy::TestAllowlist(vec![]).check(&["anyone@example.com"]).is_err());
        assert!(SendPolicy::Unrestricted.check(&["anyone@example.com"]).is_ok());
    }
}
