//! Accounts. Passwords go to the OS keyring and never come back out.

use mailcore::auth;
use mailcore::models::NewAccount;
use mailcore::store::{accounts, folders, settings};

use crate::db::shared_db;

/// Every account as JSON, for the account switcher.
pub fn accounts_json() -> anyhow::Result<String> {
    Ok(mailcore::feed::accounts_json(shared_db()?)?)
}

/// One account as a JSON form for the edit dialog.
///
/// Never carries a password: secrets live in the keyring and the UI can only
/// overwrite them, never read them back. An empty password on save therefore
/// means "keep the stored one" (see [`save_account`]).
pub fn account_form(id: i64) -> anyhow::Result<String> {
    let a = accounts::get(shared_db()?, id)?;
    Ok(serde_json::json!({
        "id": a.id,
        "name": a.name,
        "email": a.email_address,
        "from_name": a.from_name,
        "imap_host": a.imap_host,
        "imap_port": a.imap_port.to_string(),
        "imap_sec": a.imap_security,
        "imap_user": a.imap_username,
        "smtp_host": a.smtp_host,
        "smtp_port": a.smtp_port.to_string(),
        "smtp_sec": a.smtp_security,
        "smtp_user": a.smtp_username,
    })
    .to_string())
}

/// Create or update an account from the setup dialog's JSON form
/// (`{name, email, from_name?, imap_host, imap_port, imap_sec, imap_user,
/// password, smtp_host, smtp_port, smtp_sec, smtp_user, smtp_password?}`).
///
/// Keyed by email address, like the Qt frontend: re-saving a known address
/// edits that account (and migrates its secrets into the current keyring
/// backend) instead of creating a duplicate. Returns the account id.
pub fn save_account(form: String) -> anyhow::Result<i64> {
    let v: serde_json::Value =
        serde_json::from_str(&form).map_err(|_| anyhow::anyhow!("invalid account form"))?;
    let text = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    // Ports arrive as strings from a text field, but a Dart int is just as
    // plausible from a preset — accept either rather than silently defaulting.
    let port = |k: &str, dflt: u16| -> u16 {
        v.get(k)
            .and_then(|x| {
                x.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| x.as_u64().and_then(|n| u16::try_from(n).ok()))
            })
            .unwrap_or(dflt)
    };

    let email = text("email");
    let imap_host = text("imap_host");
    let smtp_host = text("smtp_host");
    let password = text("password");
    let imap_user = text("imap_user");
    if email.is_empty() || imap_host.is_empty() {
        anyhow::bail!("fill email and IMAP host");
    }
    if smtp_host.is_empty() {
        anyhow::bail!("fill the SMTP host");
    }
    let smtp_user = match text("smtp_user") {
        s if s.is_empty() => imap_user.clone(),
        s => s,
    };
    let name = match text("name") {
        s if s.is_empty() => email.clone(),
        s => s,
    };

    let db = shared_db()?;
    let draft = NewAccount {
        name,
        email_address: email.clone(),
        from_name: text("from_name"),
        imap_host,
        imap_port: port("imap_port", 993),
        imap_security: text("imap_sec"),
        imap_username: imap_user,
        smtp_host,
        smtp_port: port("smtp_port", 465),
        smtp_security: text("smtp_sec"),
        smtp_username: smtp_user,
        auth_vault_key: String::new(), // filled in below
        check_interval_secs: 300,
    };

    let existing = accounts::list(db)?
        .into_iter()
        .find(|a| a.email_address == email);
    let id = match existing {
        Some(existing) => {
            accounts::update_connection(db, existing.id, &draft)?;
            // Host, user or password may have changed: drop the pooled
            // session so the next action connects with the new values.
            crate::session::evict_session(existing.id);
            // A blank password on an edit keeps the stored secret — the
            // dialog never shows it, so re-typing must not be required.
            if !password.is_empty() {
                auth::save_account_secrets(
                    &existing.auth_vault_key,
                    &password,
                    &text("smtp_password"),
                )
                .map_err(|e| anyhow::anyhow!("keyring unavailable: {e}"))?;
            }
            existing.id
        }
        None => {
            if password.is_empty() {
                anyhow::bail!("a password is required for a new account");
            }
            let vault = auth::new_vault_key();
            auth::save_account_secrets(&vault, &password, &text("smtp_password"))
                .map_err(|e| anyhow::anyhow!("keyring unavailable: {e}"))?;
            let with_vault = NewAccount {
                auth_vault_key: vault,
                ..draft
            };
            accounts::create(db, &with_vault)?
        }
    };
    settings::set_last_active_account_id(db, id)?;
    Ok(id)
}

/// Delete an account with its folders, messages and keyring secrets.
///
/// Returns the account that should be shown instead, or `-1` when that was
/// the last one.
pub fn delete_account(id: i64) -> anyhow::Result<i64> {
    let db = shared_db()?;
    let account = accounts::get(db, id)?;
    // Drop the keyring entry first: if the row went away and this failed, the
    // secret would be orphaned with nothing left pointing at it. A keyring we
    // cannot reach must still not block the delete, so this only warns — the
    // leftover is visible and removable in the OS keyring UI.
    if let Err(e) = auth::delete_account_secrets(&account.auth_vault_key) {
        log::warn!(
            "accounts: keyring entry for {} not removed: {e}",
            account.email_address
        );
    }
    accounts::delete(db, id)?;
    // The account is gone; don't keep a socket authenticated as it.
    crate::session::evict_session(id);

    let next = accounts::list(db)?.first().map(|a| a.id).unwrap_or(-1);
    settings::set_last_active_account_id(db, next.max(0))?;
    Ok(next)
}

/// The account the app should open on, and the folder to land in.
///
/// Restored from the settings store so the app comes back where it was left;
/// falls back to the first account, and within it to the inbox. Either id is
/// `-1` when there is nothing to select.
pub fn initial_selection() -> anyhow::Result<Selection> {
    let db = shared_db()?;
    let list = accounts::list(db)?;
    let wanted = settings::get_last_active_account_id(db).unwrap_or(-1);
    let Some(account) = list
        .iter()
        .find(|a| a.id == wanted)
        .or_else(|| list.first())
    else {
        return Ok(Selection {
            account_id: -1,
            folder_id: -1,
        });
    };
    Ok(Selection {
        account_id: account.id,
        folder_id: default_folder_id(db, account.id),
    })
}

/// Switch the active account and report the folder to land in.
pub fn select_account(id: i64) -> anyhow::Result<Selection> {
    let db = shared_db()?;
    let account = accounts::get(db, id)?;
    settings::set_last_active_account_id(db, account.id)?;
    Ok(Selection {
        account_id: account.id,
        folder_id: default_folder_id(db, account.id),
    })
}

/// An account plus the folder to show in it. `-1` means "nothing".
#[flutter_rust_bridge::frb]
#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub account_id: i64,
    pub folder_id: i64,
}

/// Inbox if the account has one, else its first folder, else nothing.
fn default_folder_id(db: &mailcore::Db, account_id: i64) -> i64 {
    let folders = folders::list_by_account(db, account_id).unwrap_or_default();
    folders
        .iter()
        .find(|f| f.role == mailcore::models::FolderRole::Inbox)
        .or_else(|| folders.first())
        .map(|f| f.id)
        .unwrap_or(-1)
}
