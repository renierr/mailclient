//! The About view's server-capability payload: one shape from both adapters
//! (was a twin — `mailapp` carried the error in the payload while `mailffi`
//! failed the job, which lost the account match on the native side). Jobs
//! always finish with the error in the payload; both frontends read `error`
//! from it.

use crate::models::Account;

/// The payload for a finished lookup: the account, its capability list, and
/// `""` for no error.
#[must_use]
pub fn payload(acc: &Account, caps: &[String]) -> String {
    serde_json::json!({
        "account_id": acc.id,
        "email": acc.email_address,
        "imap_host": acc.imap_host,
        "imap_port": acc.imap_port,
        "capabilities": caps,
        "error": "",
    })
    .to_string()
}

/// The payload when the lookup never reached the server: the requested id
/// with empty account fields, so the frontend still matches its request.
#[must_use]
pub fn error_payload(account_id: i64, error: &str) -> String {
    serde_json::json!({
        "account_id": account_id,
        "email": "",
        "imap_host": "",
        "imap_port": 0,
        "capabilities": Vec::<String>::new(),
        "error": error,
    })
    .to_string()
}

/// The payload when the account is known but the lookup failed: the
/// account's fields stay, so the frontend still shows which server failed.
#[must_use]
pub fn failed_payload(acc: &Account, error: &str) -> String {
    serde_json::json!({
        "account_id": acc.id,
        "email": acc.email_address,
        "imap_host": acc.imap_host,
        "imap_port": acc.imap_port,
        "capabilities": Vec::<String>::new(),
        "error": error,
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> Account {
        Account {
            id: 7,
            name: "Test".to_string(),
            email_address: "me@example.com".to_string(),
            from_name: String::new(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            imap_security: "ssl".to_string(),
            imap_username: "me".to_string(),
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 465,
            smtp_security: "ssl".to_string(),
            smtp_username: "me".to_string(),
            auth_vault_key: "k".to_string(),
            check_interval_secs: 900,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn payload_carries_caps_with_an_empty_error() {
        let v: serde_json::Value =
            serde_json::from_str(&payload(&account(), &["IMAP4rev1".to_string()])).unwrap();
        assert_eq!(v["account_id"], 7);
        assert_eq!(v["capabilities"], serde_json::json!(["IMAP4rev1"]));
        assert_eq!(v["error"], "");
    }

    #[test]
    fn error_payload_keeps_the_requested_id() {
        let v: serde_json::Value = serde_json::from_str(&error_payload(9, "boom")).unwrap();
        assert_eq!(v["account_id"], 9);
        assert_eq!(v["email"], "");
        assert!(v["capabilities"].as_array().unwrap().is_empty());
        assert_eq!(v["error"], "boom");
    }

    #[test]
    fn failed_payload_keeps_the_account_fields() {
        let v: serde_json::Value =
            serde_json::from_str(&failed_payload(&account(), "down")).unwrap();
        assert_eq!(v["account_id"], 7);
        assert_eq!(v["email"], "me@example.com");
        assert_eq!(v["imap_host"], "imap.example.com");
        assert_eq!(v["error"], "down");
    }
}
