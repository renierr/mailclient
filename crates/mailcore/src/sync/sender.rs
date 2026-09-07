//! Outbound sending via SMTP (Milestone 2).
//!
//! M0 defines the shape: the queue worker will call [`SmtpSender`] with the
//! account config; the actual `lettre` transport wiring lands in M2 alongside
//! the composer. Passwords arrive as function args (from the OS keyring),
//! never from SQLite.

use crate::error::Result;
use crate::models::Account;
use crate::sync::traits::MailSender;

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
    /// Build from account settings (password supplied per-send from keyring).
    #[must_use]
    pub fn new(account: &Account) -> Self {
        Self {
            endpoint: endpoint_for(account),
            username: account.smtp_username.clone(),
            from: account.email_address.clone(),
        }
    }
}

impl MailSender for SmtpSender {
    fn send_queued(&mut self, _account_id: i64, _message_id: Option<i64>) -> Result<String> {
        log::info!(
            "M2: would send via {} as {} <{}>",
            self.endpoint.addr,
            self.username,
            self.from
        );
        // Placeholder Message-ID until lettre builds the real MIME message.
        Ok(format!("<{}@mailclient.local>", uuid::Uuid::new_v4()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_prefers_implicit_tls_on_465() {
        let a = Account {
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
        };
        let ep = endpoint_for(&a);
        assert_eq!(ep.addr, "smtp.x:587");
        assert!(!ep.implicit_tls);
    }
}
