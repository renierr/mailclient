//! Shared test fixtures (`cfg(test)` only).

use crate::models::Account;

/// Minimal account for sender unit tests (no network, no keyring).
pub(crate) fn test_account() -> Account {
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
