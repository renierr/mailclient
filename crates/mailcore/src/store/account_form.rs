//! Saving an account from a setup-dialog form.
//!
//! Both frontends used to parse, validate and store this form themselves, and
//! the two copies had already drifted: the Qt bridge edited by id and rejected
//! a duplicate address, the Flutter bridge keyed only on email and accepted a
//! port as either a string or an int. The union of those lives here, so a fix
//! lands once. Each bridge only translates the result into its own UI.

use serde_json::Value;

mod fields;

pub use fields::{
    check, default_port, defaults_json, guess, is_plaintext, normalize_security,
    port_after_security_change, FormCheck, Protocol, ServerGuess, SECURITY_CHOICES,
};

use crate::auth::{self, AccountSecrets};
use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Account, NewAccount};
use crate::store::{accounts, settings};
use crate::sync::pool;

/// Where an account's passwords go. The real one is the OS keyring; tests
/// pass an in-memory stand-in so this never dials out or touches a secret
/// store.
pub trait SecretStore {
    /// Store both passwords. An empty SMTP password means "same as IMAP".
    fn save(&mut self, vault_key: &str, imap_password: &str, smtp_password: &str) -> Result<()>;
    /// Both passwords as stored, the SMTP one unresolved (empty = same as
    /// IMAP).
    fn load(&mut self, vault_key: &str) -> Result<AccountSecrets>;
    /// Remove an entry, used to undo a save whose account row then failed.
    fn delete(&mut self, vault_key: &str) -> Result<()>;
}

/// The OS keyring (or the Android app-private vault).
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn save(&mut self, vault_key: &str, imap_password: &str, smtp_password: &str) -> Result<()> {
        auth::save_account_secrets(vault_key, imap_password, smtp_password)
    }

    fn load(&mut self, vault_key: &str) -> Result<AccountSecrets> {
        auth::load_stored_secrets(vault_key)
    }

    fn delete(&mut self, vault_key: &str) -> Result<()> {
        auth::delete_account_secrets(vault_key)
    }
}

/// Create or update an account from the setup dialog's JSON form.
///
/// An `id` edits that account, so changing its address renames it instead of
/// creating a second one; no id creates one. An address another account
/// already uses is refused either way (case-insensitively). On an edit each
/// blank password keeps its stored secret — the dialog never shows them, so
/// re-typing must not be required. A new account requires the IMAP one.
/// Returns the account id. Show errors with [`user_message`].
pub fn save(db: &Db, form: &str, secrets: &mut dyn SecretStore) -> Result<i64> {
    let v: Value = serde_json::from_str(form).map_err(|_| invalid("invalid account form"))?;
    let raw = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let text = |k: &str| raw(k).trim().to_string();
    let edit_id = v.get("id").and_then(Value::as_i64).filter(|id| *id >= 0);
    // The same check the forms show inline; the first problem is the error.
    if let Some((_, msg)) = check(&v, edit_id.is_some()).errors.into_iter().next() {
        return Err(invalid(&msg));
    }

    let email = text("email");
    let imap_security = normalize_security(&raw("imap_sec"));
    let smtp_security = normalize_security(&raw("smtp_sec"));
    let port = |k: &str, protocol: Protocol, security: &str| -> Result<u16> {
        fields::port_value(v.get(k))
            .map(|p| p.unwrap_or_else(|| default_port(protocol, security)))
            .map_err(|()| invalid("a port must be a number from 1 to 65535"))
    };
    // Passwords are taken verbatim: a leading or trailing space is part of
    // the secret.
    let password = raw("password");
    let smtp_password = raw("smtp_password");
    let imap_user = match text("imap_user") {
        ref s if s.is_empty() => email.clone(),
        s => s,
    };
    let smtp_user = match text("smtp_user") {
        ref s if s.is_empty() => imap_user.clone(),
        s => s,
    };
    let name = match text("name") {
        ref s if s.is_empty() => email.clone(),
        s => s,
    };

    let draft = NewAccount {
        name,
        email_address: email.clone(),
        from_name: text("from_name"),
        imap_host: text("imap_host"),
        imap_port: port("imap_port", Protocol::Imap, imap_security)?,
        imap_security: imap_security.to_string(),
        imap_username: imap_user,
        smtp_host: text("smtp_host"),
        smtp_port: port("smtp_port", Protocol::Smtp, smtp_security)?,
        smtp_security: smtp_security.to_string(),
        smtp_username: smtp_user,
        auth_vault_key: String::new(),
        check_interval_secs: 300,
    };

    let all = accounts::list(db)?;
    if let Some(other) = all
        .iter()
        .find(|a| a.email_address.eq_ignore_ascii_case(&email) && Some(a.id) != edit_id)
    {
        return Err(invalid(&format!(
            "another account already uses {}",
            other.email_address
        )));
    }

    let id = match edit_id {
        Some(id) => {
            let existing = all
                .into_iter()
                .find(|a| a.id == id)
                .ok_or_else(|| invalid("this account no longer exists"))?;
            save_edit(db, secrets, &existing, &draft, &password, &smtp_password)?;
            existing.id
        }
        None => create_new(db, secrets, draft, &password, &smtp_password)?,
    };
    settings::set_last_active_account_id(db, id)?;
    Ok(id)
}

/// One account as the edit form's JSON. Never carries a password: secrets
/// live in the keyring and the form can only overwrite them, so a blank
/// password on [`save`] keeps the stored one.
pub fn load(db: &Db, id: i64) -> Result<String> {
    let a = accounts::get(db, id)?;
    Ok(serde_json::json!({
        "id": a.id,
        "name": a.name,
        "email": a.email_address,
        "from_name": a.from_name,
        "imap_host": a.imap_host,
        "imap_port": a.imap_port.to_string(),
        "imap_sec": normalize_security(&a.imap_security),
        "imap_user": a.imap_username,
        "smtp_host": a.smtp_host,
        "smtp_port": a.smtp_port.to_string(),
        "smtp_sec": normalize_security(&a.smtp_security),
        "smtp_user": a.smtp_username,
    })
    .to_string())
}

fn invalid(msg: &str) -> StoreError {
    StoreError::InvalidInput(msg.to_string())
}

/// Text of a [`save`] error for the form, without the `invalid input:`
/// prefix the error's `Display` adds.
#[must_use]
pub fn user_message(e: &StoreError) -> String {
    match e {
        StoreError::InvalidInput(m) => m.clone(),
        other => other.to_string(),
    }
}

fn save_edit(
    db: &Db,
    secrets: &mut dyn SecretStore,
    existing: &Account,
    draft: &NewAccount,
    password: &str,
    smtp_password: &str,
) -> Result<()> {
    // Secrets first: if the keyring refuses, nothing has changed yet, instead
    // of new connection details sitting next to the old password.
    if !password.is_empty() || !smtp_password.is_empty() {
        // Each blank field keeps its stored value. An unreadable entry only
        // matters when the IMAP password is not being replaced.
        let stored = secrets.load(&existing.auth_vault_key);
        let imap = if password.is_empty() {
            match &stored {
                Ok(s) => s.imap_password.clone(),
                Err(_) => {
                    return Err(invalid(
                        "enter the IMAP password too: the stored one cannot be read",
                    ))
                }
            }
        } else {
            password.to_string()
        };
        let smtp = if smtp_password.is_empty() {
            stored.map(|s| s.smtp_password).unwrap_or_default()
        } else {
            smtp_password.to_string()
        };
        secrets.save(&existing.auth_vault_key, &imap, &smtp)?;
    }
    accounts::update_connection(db, existing.id, draft)?;
    // Host, user or password may have changed: drop the pooled session so
    // the next action connects with the new values.
    pool::evict_session(existing.id);
    Ok(())
}

fn create_new(
    db: &Db,
    secrets: &mut dyn SecretStore,
    draft: NewAccount,
    password: &str,
    smtp_password: &str,
) -> Result<i64> {
    if password.is_empty() {
        return Err(invalid("a password is required for a new account"));
    }
    let vault = auth::new_vault_key();
    secrets.save(&vault, password, smtp_password)?;
    let with_vault = NewAccount {
        auth_vault_key: vault.clone(),
        ..draft
    };
    accounts::create(db, &with_vault).inspect_err(|_| {
        // No row points at the secret: do not leave it orphaned.
        if let Err(e) = secrets.delete(&vault) {
            log::warn!("orphaned keyring entry not removed: {e}");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Memory {
        saved: HashMap<String, (String, String)>,
    }

    impl SecretStore for Memory {
        fn save(
            &mut self,
            vault_key: &str,
            imap_password: &str,
            smtp_password: &str,
        ) -> Result<()> {
            self.saved.insert(
                vault_key.to_string(),
                (imap_password.to_string(), smtp_password.to_string()),
            );
            Ok(())
        }

        fn load(&mut self, vault_key: &str) -> Result<AccountSecrets> {
            self.saved
                .get(vault_key)
                .map(|(i, s)| AccountSecrets {
                    imap_password: i.clone(),
                    smtp_password: s.clone(),
                })
                .ok_or_else(|| StoreError::Keyring("missing".into()))
        }

        fn delete(&mut self, vault_key: &str) -> Result<()> {
            self.saved.remove(vault_key);
            Ok(())
        }
    }

    fn form(extra: &str) -> String {
        format!(
            r#"{{"name":"Work","email":"user@example.com","imap_host":"imap.example.com",
            "imap_port":"993","smtp_host":"smtp.example.com","password":"s3cret"{extra}}}"#
        )
    }

    fn stored(db: &Db, secrets: &Memory, id: i64) -> (String, String) {
        let key = accounts::get(db, id).unwrap().auth_vault_key;
        secrets.saved.get(&key).unwrap().clone()
    }

    #[test]
    fn a_new_account_stores_the_password_and_a_blank_edit_keeps_it() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory::default();
        let id = save(&db, &form(""), &mut secrets).unwrap();
        assert_eq!(stored(&db, &secrets, id).0, "s3cret");

        let edited = form(&format!(r#","id":{id},"password":"","imap_port":143"#));
        assert_eq!(save(&db, &edited, &mut secrets).unwrap(), id);
        assert_eq!(accounts::get(&db, id).unwrap().imap_port, 143);
        assert_eq!(stored(&db, &secrets, id), ("s3cret".into(), String::new()));
    }

    #[test]
    fn each_blank_password_keeps_its_own_stored_value() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory::default();
        let id = save(&db, &form(r#","smtp_password":"smtp1""#), &mut secrets).unwrap();

        // Only the SMTP password changes.
        let smtp_only = form(&format!(
            r#","id":{id},"password":"","smtp_password":"smtp2""#
        ));
        save(&db, &smtp_only, &mut secrets).unwrap();
        assert_eq!(stored(&db, &secrets, id), ("s3cret".into(), "smtp2".into()));

        // Only the IMAP password changes; the separate SMTP one survives.
        let imap_only = form(&format!(r#","id":{id},"password":"new""#));
        save(&db, &imap_only, &mut secrets).unwrap();
        assert_eq!(stored(&db, &secrets, id), ("new".into(), "smtp2".into()));
    }

    #[test]
    fn passwords_keep_surrounding_spaces() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory::default();
        let id = save(&db, &form(r#","password":" pw ""#), &mut secrets).unwrap();
        assert_eq!(stored(&db, &secrets, id).0, " pw ");
    }

    #[test]
    fn an_id_renames_instead_of_creating_a_second_account() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory::default();
        let id = save(&db, &form(""), &mut secrets).unwrap();
        let renamed = format!(
            r#"{{"id":{id},"email":"other@example.com","imap_host":"imap.example.com","smtp_host":"smtp.example.com","password":"new"}}"#
        );
        assert_eq!(save(&db, &renamed, &mut secrets).unwrap(), id);
        assert_eq!(accounts::list(&db).unwrap().len(), 1);
        assert_eq!(
            accounts::get(&db, id).unwrap().email_address,
            "other@example.com"
        );
    }

    #[test]
    fn a_port_given_as_a_number_is_accepted_and_nonsense_is_refused() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory::default();
        let json = r#"{"email":"user@example.com","imap_host":"imap.example.com","imap_port":143,"smtp_host":"smtp.example.com","smtp_port":587,"password":"s"}"#;
        let id = save(&db, json, &mut secrets).unwrap();
        let a = accounts::get(&db, id).unwrap();
        assert_eq!((a.imap_port, a.smtp_port), (143, 587));

        let bad = form(&format!(r#","id":{id},"imap_port":"99999""#));
        assert!(matches!(
            save(&db, &bad, &mut secrets),
            Err(StoreError::InvalidInput(_))
        ));
    }

    #[test]
    fn an_address_another_account_already_uses_is_refused() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory::default();
        save(&db, &form(""), &mut secrets).unwrap();

        // Adding the same address again, in any case, is not an edit.
        let again = form(r#","email":"User@Example.com""#);
        let err = save(&db, &again, &mut secrets).unwrap_err();
        assert_eq!(
            user_message(&err),
            "another account already uses user@example.com"
        );

        let other_id = save(&db, &form(r#","email":"other@example.com""#), &mut secrets).unwrap();
        let clash = form(&format!(r#","id":{other_id}"#));
        assert!(save(&db, &clash, &mut secrets).is_err());
        assert_eq!(accounts::list(&db).unwrap().len(), 2);
        assert_eq!(secrets.saved.len(), 2, "a refused save stores no secret");
    }

    #[test]
    fn a_new_account_without_a_password_is_refused() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory::default();
        let err = save(&db, &form(r#","password":"""#), &mut secrets).unwrap_err();
        assert!(matches!(err, StoreError::InvalidInput(_)));
        assert!(accounts::list(&db).unwrap().is_empty());
    }

    #[test]
    fn a_blank_imap_user_is_the_address_and_security_is_normalized() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory::default();
        let id = save(
            &db,
            &form(r#","imap_sec":"SSL","smtp_sec":"plain","smtp_port":"""#),
            &mut secrets,
        )
        .unwrap();
        let a = accounts::get(&db, id).unwrap();
        assert_eq!(a.imap_username, "user@example.com");
        assert_eq!(a.smtp_username, "user@example.com");
        assert_eq!(
            (a.imap_security.as_str(), a.smtp_security.as_str()),
            ("tls", "none")
        );
        assert_eq!(
            a.smtp_port, 587,
            "a blank port is the usual one for its security"
        );

        let loaded: Value = serde_json::from_str(&load(&db, id).unwrap()).unwrap();
        assert_eq!(loaded["smtp_sec"], "none");
        assert_eq!(loaded["imap_user"], "user@example.com");
        assert!(loaded.get("password").is_none());
    }
}
