//! OS-keyring access for account secrets.
//!
//! The SQLite DB stores only `auth_vault_key` (a random `mailclient:<uuid>`
//! reference); passwords/tokens live here. Nothing secret ever touches the DB,
//! logs, docs, or git.

use crate::error::{Result, StoreError};

const SERVICE: &str = "mailclient";

/// Generate a fresh vault key for a new account.
#[must_use]
pub fn new_vault_key() -> String {
    format!("mailclient:{}", uuid::Uuid::new_v4())
}

/// Store a secret under a vault key.
pub fn save_secret(vault_key: &str, secret: &str) -> Result<()> {
    keyring::Entry::new(SERVICE, vault_key)
        .map_err(|e| StoreError::InvalidInput(format!("keyring unavailable: {e}")))?
        .set_password(secret)
        .map_err(|e| StoreError::InvalidInput(format!("keyring store failed: {e}")))
}

/// Load a secret by vault key.
pub fn load_secret(vault_key: &str) -> Result<String> {
    keyring::Entry::new(SERVICE, vault_key)
        .map_err(|e| StoreError::InvalidInput(format!("keyring unavailable: {e}"))?
        .get_password()
        .map_err(|e| StoreError::InvalidInput(format!("keyring load failed: {e}")))
}

/// Delete a secret (account removal).
pub fn delete_secret(vault_key: &str) -> Result<()> {
    keyring::Entry::new(SERVICE, vault_key)
        .map_err(|e| StoreError::InvalidInput(format!("keyring unavailable: {e}"))?
        .delete_credential()
        .map_err(|e| StoreError::InvalidInput(format!("keyring delete failed: {e}")))
}
