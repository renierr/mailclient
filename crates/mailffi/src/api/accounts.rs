//! Accounts. Passwords go to the OS keyring and never come back out.

use mailcore::auth;
use mailcore::store::account_form::{self, KeyringStore};
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
    Ok(account_form::load(shared_db()?, id)?)
}

/// A new account form's starting values and security choices (JSON).
#[flutter_rust_bridge::frb(sync)]
pub fn account_form_defaults() -> String {
    account_form::defaults_json()
}

/// Server guesses for a typed address as JSON (`imap_host`, `smtp_host`,
/// `imap_user`), `{}` while the address is still partial.
#[flutter_rust_bridge::frb(sync)]
pub fn account_guess(email: String) -> String {
    match account_form::guess(&email) {
        Some(g) => serde_json::json!({
            "imap_host": g.imap_host,
            "smtp_host": g.smtp_host,
            "imap_user": g.imap_user,
        }),
        None => serde_json::json!({}),
    }
    .to_string()
}

/// The port field after `protocol`'s (`imap`/`smtp`) security changed.
#[flutter_rust_bridge::frb(sync)]
pub fn account_port_for_security(
    protocol: String,
    old_sec: String,
    new_sec: String,
    port: String,
) -> String {
    match account_form::Protocol::parse(&protocol) {
        Some(p) => account_form::port_after_security_change(p, &old_sec, &new_sec, &port),
        None => port,
    }
}

/// Per-field `errors` and `warnings` for the account form (JSON), the same
/// check [`save_account`] runs.
#[flutter_rust_bridge::frb(sync)]
pub fn account_form_check(form: String, editing: bool) -> String {
    let v = serde_json::from_str(&form).unwrap_or_default();
    account_form::check(&v, editing).to_json()
}

/// Create or update an account from the setup dialog's JSON form.
///
/// The decision — edit by id, update a known address, reject a duplicate,
/// keep a blank password — lives in `mailcore::store::account_form`, shared
/// with the Qt frontend. Returns the account id.
pub fn save_account(form: String) -> anyhow::Result<i64> {
    account_form::save(shared_db()?, &form, &mut KeyringStore)
        .map_err(|e| anyhow::anyhow!(account_form::user_message(&e)))
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
    mailcore::sync::pool::evict_session(id);

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
