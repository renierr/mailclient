//! OS-keyring access for account secrets.
//!
//! The SQLite DB stores only `auth_vault_key` (a random `mailclient:<uuid>`
//! reference). Passwords live here, in the desktop Secret Service keyring
//! (GNOME Keyring / KDE Wallet), as one JSON entry per account:
//! `{"imap": "…", "smtp": "…"}`. Nothing secret ever touches the DB, logs,
//! docs, or git.

use crate::error::{Result, StoreError};

#[cfg(not(target_os = "android"))]
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
    #[cfg(not(target_os = "android"))]
    {
        entry(vault_key)?
            .set_password(&payload.to_string())
            .map_err(|e| StoreError::Keyring(format!("store failed: {e}")))?;
    }
    #[cfg(target_os = "android")]
    {
        android_vault::set_secret(vault_key, &payload.to_string())?;
    }
    Ok(())
}

/// Load both passwords; empty/missing SMTP falls back to the IMAP password.
pub fn load_account_secrets(vault_key: &str) -> Result<AccountSecrets> {
    let stored = load_stored_secrets(vault_key)?;
    Ok(AccountSecrets {
        smtp_password: resolve_smtp(&stored.imap_password, &stored.smtp_password),
        imap_password: stored.imap_password,
    })
}

/// Both passwords exactly as stored: an empty SMTP password stays empty
/// ("same as IMAP") instead of being resolved. For editing, where a blank
/// field must keep what is there rather than copy the IMAP password over.
pub fn load_stored_secrets(vault_key: &str) -> Result<AccountSecrets> {
    #[cfg(not(target_os = "android"))]
    let raw = entry(vault_key)?
        .get_password()
        .map_err(|e| StoreError::Keyring(format!("load failed: {e}")))?;
    #[cfg(target_os = "android")]
    let raw = android_vault::get_secret(vault_key)?;

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
        return Err(StoreError::Keyring(
            "account has no stored password".to_string(),
        ));
    }
    Ok(AccountSecrets {
        imap_password: imap,
        smtp_password: smtp,
    })
}

/// Load with a few quick retries.
///
/// The headless CLI (`--sync-once`) spawns fresh for every poll and opens a
/// new Secret Service D-Bus connection each time, so a momentary "remote
/// peer disconnected" should ride out instead of surfacing as an error.
/// Interactive callers keep the single-attempt `load_account_secrets`
/// (the user is present to unlock/retry).
pub async fn load_account_secrets_retry(vault_key: &str) -> Result<AccountSecrets> {
    const ATTEMPTS: usize = 3;
    let mut err = match load_account_secrets(vault_key) {
        Ok(s) => return Ok(s),
        Err(e) => e,
    };
    for _ in 1..ATTEMPTS {
        tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        match load_account_secrets(vault_key) {
            Ok(s) => return Ok(s),
            Err(e) => err = e,
        }
    }
    Err(err)
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
    #[cfg(not(target_os = "android"))]
    {
        entry(vault_key)?
            .delete_credential()
            .map_err(|e| StoreError::Keyring(format!("delete failed: {e}")))?;
    }
    #[cfg(target_os = "android")]
    {
        android_vault::delete_secret(vault_key)?;
    }
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn entry(vault_key: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, vault_key)
        .map_err(|e| StoreError::Keyring(format!("unavailable: {e}")))
}

#[cfg(target_os = "android")]
pub fn set_vault_dir(dir: std::path::PathBuf) {
    android_vault::set_vault_dir(dir);
}

#[cfg(target_os = "android")]
mod android_vault {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static VAULT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

    pub fn set_vault_dir(dir: PathBuf) {
        if let Ok(mut lock) = VAULT_DIR.lock() {
            *lock = Some(dir);
        }
    }

    fn vault_file() -> PathBuf {
        if let Ok(lock) = VAULT_DIR.lock() {
            if let Some(dir) = &*lock {
                return dir.join("auth_vault.json");
            }
        }
        if let Ok(db) = std::env::var("MAILCLIENT_DB") {
            if let Some(parent) = std::path::Path::new(&db).parent() {
                return parent.join("auth_vault.json");
            }
        }
        crate::db::default_db_path()
            .parent()
            .map(|p| p.join("auth_vault.json"))
            .unwrap_or_else(|| PathBuf::from("auth_vault.json"))
    }

    /// Serialises every read-modify-write of the vault file. The foreground
    /// app and the background check share this process, so one lock is
    /// enough.
    static VAULT_WRITE: Mutex<()> = Mutex::new(());

    /// The whole vault. A missing file is an empty vault; an unreadable or
    /// corrupt one is an error — never an empty map, or the next write would
    /// silently drop every other account's secrets.
    fn read_map(path: &Path) -> Result<HashMap<String, String>> {
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let data = fs::read_to_string(path)
            .map_err(|e| StoreError::Keyring(format!("read vault failed: {e}")))?;
        serde_json::from_str(&data).map_err(|e| StoreError::Keyring(format!("corrupt vault: {e}")))
    }

    /// Write via a temp file and rename, so a kill mid-write leaves the old
    /// vault intact instead of a truncated one.
    fn write_map(path: &Path, map: &HashMap<String, String>) -> Result<()> {
        let data = serde_json::to_string_pretty(map)
            .map_err(|e| StoreError::Keyring(format!("serialize vault failed: {e}")))?;
        let tmp = path.with_extension("json.tmp");
        let write = || -> std::io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(data.as_bytes())?;
            f.sync_all()?;
            fs::rename(&tmp, path)
        };
        write().map_err(|e| StoreError::Keyring(format!("write vault failed: {e}")))
    }

    fn update(change: impl FnOnce(&mut HashMap<String, String>)) -> Result<()> {
        let _guard = VAULT_WRITE.lock().unwrap_or_else(|p| p.into_inner());
        let path = vault_file();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut map = read_map(&path)?;
        change(&mut map);
        write_map(&path, &map)
    }

    pub fn get_secret(vault_key: &str) -> Result<String> {
        let path = vault_file();
        if !path.exists() {
            return Err(StoreError::Keyring("vault file does not exist".into()));
        }
        read_map(&path)?
            .remove(vault_key)
            .ok_or_else(|| StoreError::Keyring("key not found in vault".into()))
    }

    pub fn set_secret(vault_key: &str, secret: &str) -> Result<()> {
        update(|map| {
            map.insert(vault_key.to_string(), secret.to_string());
        })
    }

    pub fn delete_secret(vault_key: &str) -> Result<()> {
        if !vault_file().exists() {
            return Ok(());
        }
        update(|map| {
            map.remove(vault_key);
        })
    }
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
