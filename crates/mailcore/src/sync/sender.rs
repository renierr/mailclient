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

/// One outbound message (transport details come from the account).
pub struct SendRequest<'a> {
    /// Recipients (checked against the [`SendPolicy`]).
    pub to: &'a [String],
    /// Sender identity. `None` = account email. Any other address is used
    /// verbatim (server may reject logins that must match the username).
    pub from: Option<&'a str>,
    pub subject: &'a str,
    pub body_text: &'a str,
    pub policy: &'a SendPolicy,
    /// SMTP password (keyring or test env), never stored.
    pub password: &'a str,
    /// IMAP password for filing the Sent copy (if `sent_copy_enabled`).
    /// `None` skips the copy with a warning; the send still succeeds.
    pub imap_password: Option<&'a str>,
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
        let refs: Vec<&str> = req.to.iter().map(String::as_str).collect();
        req.policy.check(&refs)?;

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
        let email = builder
            .header(ContentType::TEXT_PLAIN)
            .body(req.body_text.to_string())?;

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
}
