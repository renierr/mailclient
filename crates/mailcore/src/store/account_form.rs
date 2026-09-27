//! Saving an account from a setup-dialog form.
//!
//! Both frontends used to parse, validate and store this form themselves, and
//! the two copies had already drifted: the Qt bridge edited by id and rejected
//! a duplicate address, the Flutter bridge keyed only on email and accepted a
//! port as either a string or an int. The union of those lives here, so a fix
//! lands once. Each bridge only translates the result into its own UI.

use serde_json::Value;

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
}

/// The OS keyring (or the Android app-private vault).
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn save(&mut self, vault_key: &str, imap_password: &str, smtp_password: &str) -> Result<()> {
        crate::auth::save_account_secrets(vault_key, imap_password, smtp_password)
    }
}

/// Create or update an account from the setup dialog's JSON form.
///
/// An `id` edits that account, so changing its address renames it instead of
/// creating a second one. Without an id, a known email address updates that
/// account. Either way a blank password keeps the stored secret — the dialog
/// never shows it, so re-typing must not be required. A new account requires
/// one. Returns the account id.
pub fn save(db: &Db, form: &str, secrets: &mut dyn SecretStore) -> Result<i64> {
    let v: Value = serde_json::from_str(form)
        .map_err(|_| StoreError::InvalidInput("invalid account form".into()))?;
    let text = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    // Ports arrive as strings from a text field, but a numeric preset is just
    // as plausible — accept either rather than silently defaulting.
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
        return Err(StoreError::InvalidInput("fill email and IMAP host".into()));
    }
    if smtp_host.is_empty() {
        return Err(StoreError::InvalidInput("fill the SMTP host".into()));
    }
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
        imap_host,
        imap_port: port("imap_port", 993),
        imap_security: text("imap_sec"),
        imap_username: imap_user,
        smtp_host,
        smtp_port: port("smtp_port", 465),
        smtp_security: text("smtp_sec"),
        smtp_username: smtp_user,
        auth_vault_key: String::new(),
        check_interval_secs: 300,
    };

    // An edit names its account by id. Without one, re-saving a known email
    // updates it rather than creating a duplicate.
    let edit_id = v.get("id").and_then(Value::as_i64).filter(|id| *id >= 0);
    let all = accounts::list(db)?;
    if let Some(other) = all
        .iter()
        .find(|a| a.email_address == email && edit_id.is_some_and(|id| a.id != id))
    {
        return Err(StoreError::InvalidInput(format!(
            "another account already uses {}",
            other.email_address
        )));
    }
    let target = match edit_id {
        Some(id) => Some(
            all.into_iter()
                .find(|a| a.id == id)
                .ok_or_else(|| StoreError::NotFound("this account no longer exists".into()))?,
        ),
        None => all.into_iter().find(|a| a.email_address == email),
    };

    let id = match target {
        Some(existing) => {
            save_edit(
                db,
                secrets,
                &existing,
                &draft,
                &password,
                &text("smtp_password"),
            )?;
            existing.id
        }
        None => create_new(db, secrets, draft, &password, &text("smtp_password"))?,
    };
    settings::set_last_active_account_id(db, id)?;
    Ok(id)
}

fn save_edit(
    db: &Db,
    secrets: &mut dyn SecretStore,
    existing: &Account,
    draft: &NewAccount,
    password: &str,
    smtp_password: &str,
) -> Result<()> {
    accounts::update_connection(db, existing.id, draft)?;
    // Host, user or password may have changed: drop the pooled session so
    // the next action connects with the new values.
    pool::evict_session(existing.id);
    // A blank password keeps the stored secret.
    if !password.is_empty() {
        secrets.save(&existing.auth_vault_key, password, smtp_password)?;
    }
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
        return Err(StoreError::InvalidInput(
            "a password is required for a new account".into(),
        ));
    }
    let vault = crate::auth::new_vault_key();
    secrets.save(&vault, password, smtp_password)?;
    let with_vault = NewAccount {
        auth_vault_key: vault,
        ..draft
    };
    accounts::create(db, &with_vault)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
    }

    fn form(extra: &str) -> String {
        format!(
            r#"{{"name":"Work","email":"user@example.com","imap_host":"imap.example.com",
            "imap_port":"993","smtp_host":"smtp.example.com","password":"s3cret"{extra}}}"#
        )
    }

    #[test]
    fn a_new_account_stores_the_password_and_a_blank_edit_keeps_it() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory {
            saved: HashMap::new(),
        };
        let id = save(&db, &form(""), &mut secrets).unwrap();
        let created = accounts::get(&db, id).unwrap();
        assert_eq!(
            secrets.saved.get(&created.auth_vault_key).unwrap().0,
            "s3cret"
        );

        let edited = form(r#","password":"","imap_port":143"#);
        let again = save(&db, &edited, &mut secrets).unwrap();
        assert_eq!(again, id);
        assert_eq!(accounts::get(&db, id).unwrap().imap_port, 143);
        assert_eq!(
            secrets.saved.len(),
            1,
            "a blank password must not be stored"
        );
    }

    #[test]
    fn an_id_renames_instead_of_creating_a_second_account() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory {
            saved: HashMap::new(),
        };
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
    fn a_port_given_as_a_number_is_accepted() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory {
            saved: HashMap::new(),
        };
        let json = r#"{"email":"user@example.com","imap_host":"imap.example.com","imap_port":143,"smtp_host":"smtp.example.com","smtp_port":587,"password":"s"}"#;
        let id = save(&db, json, &mut secrets).unwrap();
        let a = accounts::get(&db, id).unwrap();
        assert_eq!((a.imap_port, a.smtp_port), (143, 587));
    }

    #[test]
    fn an_address_another_account_already_uses_is_refused() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory {
            saved: HashMap::new(),
        };
        save(&db, &form(""), &mut secrets).unwrap();
        let other = form(r#","email":"other@example.com""#);
        let other_id = save(&db, &other, &mut secrets).unwrap();
        let clash = format!(
            r#"{{"id":{other_id},"email":"user@example.com","imap_host":"imap.example.com","smtp_host":"smtp.example.com","password":"s"}}"#
        );
        let err = save(&db, &clash, &mut secrets).unwrap_err();
        assert!(matches!(err, StoreError::InvalidInput(_)));
        assert_eq!(accounts::list(&db).unwrap().len(), 2);
    }

    #[test]
    fn a_new_account_without_a_password_is_refused() {
        let db = Db::open_in_memory().unwrap();
        let mut secrets = Memory {
            saved: HashMap::new(),
        };
        let err = save(&db, &form(r#","password":"""#), &mut secrets).unwrap_err();
        assert!(matches!(err, StoreError::InvalidInput(_)));
        assert!(accounts::list(&db).unwrap().is_empty());
    }
}
