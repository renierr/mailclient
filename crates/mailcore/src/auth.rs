//! OS-keyring access for account secrets.
//!
//! The SQLite DB stores only `auth_vault_key` (a random `mailclient:<uuid>`
//! reference). Passwords live here, in the desktop Secret Service keyring
//! (GNOME Keyring / KDE Wallet), as one JSON entry per account:
//! `{"imap": "…", "smtp": "…"}`. Nothing secret ever touches the DB, logs,
//! docs, or git.

use crate::error::{Result, StoreError};

const SERVICE: &str = "mailclient";

/// Generate a fresh vault key for a new account.
#[must_use]
pub fn new_vault_key() -> String {
    format!("mailclient:{}", uuid::Uuid::new_v4())
}

/// Both passwords of an account. Empty SMTP falls back to IMAP on load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSecrets {
    pub imap_password: String,
    pub smtp_password: String,
}

/// Store both passwords under a vault key. An empty SMTP password means
/// "same as IMAP" (resolved on load).
pub fn save_account_secrets(
    vault_key: &str,
    imap_password: &str,
    smtp_password: &str,
) -> Result<()> {
    let payload = serde_json::json!({"imap": imap_password, "smtp": smtp_password});
    entry(vault_key)?
        .set_password(&payload.to_string())
        .map_err(|e| StoreError::InvalidInput(format!("keyring store failed: {e}")))
}

/// Load both passwords; empty/missing SMTP falls back to the IMAP password.
pub fn load_account_secrets(vault_key: &str) -> Result<AccountSecrets> {
    let raw = entry(vault_key)?
        .get_password()
        .map_err(|e| StoreError::InvalidInput(format!("keyring load failed: {e}")))?;
    // Legacy plain-password entries (M1 harness era): treat whole value as IMAP.
    let (imap, smtp) = match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => (
            v.get("imap")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            v.get("smtp")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        ),
        Err(_) => (raw, String::new()),
    };
    if imap.is_empty() {
        return Err(StoreError::InvalidInput(
            "empty password in keyring".to_string(),
        ));
    }
    Ok(AccountSecrets {
        smtp_password: resolve_smtp(&imap, &smtp),
        imap_password: imap,
    })
}

/// Empty SMTP password means "same as IMAP".
fn resolve_smtp(imap_password: &str, smtp_password: &str) -> String {
    if smtp_password.is_empty() {
        imap_password.to_string()
    } else {
        smtp_password.to_string()
    }
}

/// Delete an account's secrets (account removal).
pub fn delete_account_secrets(vault_key: &str) -> Result<()> {
    entry(vault_key)?
        .delete_credential()
        .map_err(|e| StoreError::InvalidInput(format!("keyring delete failed: {e}")))
}

fn entry(vault_key: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, vault_key)
        .map_err(|e| StoreError::InvalidInput(format!("keyring unavailable: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smtp_falls_back_to_imap() {
        assert_eq!(resolve_smtp("a", ""), "a");
        assert_eq!(resolve_smtp("a", "b"), "b");
    }
}
