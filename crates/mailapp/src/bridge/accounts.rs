use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::auth;
use mailcore::models::NewAccount;
use mailcore::store::{accounts, folders, settings};

use crate::bridge::qobject;
use crate::bridge::session::evict_imap_session;
use crate::bridge::{push_feeds, qstring, shared_db, DEFAULT_MESSAGE_LIMIT};

impl qobject::Bridge {
    pub fn refresh_accounts(mut self: Pin<&mut Self>) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let list = match accounts::list(db) {
            Ok(l) => l,
            Err(e) => return qstring(&e.to_string()),
        };
        self.as_mut().set_account_count(list.len() as i32);
        let wanted = settings::get_last_active_account_id(db).unwrap_or(*self.current_account_id());
        let Some(acc) = list
            .into_iter()
            .find(|a| a.id == wanted)
            .or_else(|| accounts::list(db).ok().and_then(|l| l.into_iter().next()))
        else {
            push_feeds(&mut self, db, -1, -1);
            return qstring("no account — add one first");
        };
        // Prefer inbox, else first folder.
        let folder_id = folders::list_by_account(db, acc.id)
            .ok()
            .and_then(|fs| {
                fs.iter()
                    .find(|f| f.role == mailcore::models::FolderRole::Inbox)
                    .or_else(|| fs.first())
                    .map(|f| f.id)
            })
            .unwrap_or(-1);
        // Fresh account context: restart paging from the first page.
        self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
        push_feeds(&mut self, db, acc.id, folder_id);
        let _ = settings::set_last_active_account_id(db, acc.id);
        qstring("")
    }

    pub fn add_account(mut self: Pin<&mut Self>, form: &QString) -> QString {
        let v: serde_json::Value = match serde_json::from_str(&form.to_string()) {
            Ok(v) => v,
            Err(_) => return qstring("invalid account form"),
        };
        let str_field = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let u16_field = |k: &str, dflt: u16| {
            v.get(k)
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(dflt)
        };
        let name = str_field("name");
        let email = str_field("email");
        let imap_host = str_field("imap_host");
        let imap_user = str_field("imap_user");
        let password = str_field("password");
        let smtp_host = str_field("smtp_host");
        let mut smtp_user = str_field("smtp_user");
        if email.is_empty() || imap_host.is_empty() {
            return qstring("fill email and IMAP host");
        }
        if smtp_host.is_empty() {
            return qstring("fill the SMTP host");
        }
        if smtp_user.is_empty() {
            smtp_user.clone_from(&imap_user);
        }
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let account_name = if name.is_empty() { email.clone() } else { name };
        let form_account = NewAccount {
            name: account_name,
            email_address: email.clone(),
            from_name: str_field("from_name"),
            imap_host,
            imap_port: u16_field("imap_port", 993),
            imap_security: str_field("imap_sec"),
            imap_username: imap_user,
            smtp_host,
            smtp_port: u16_field("smtp_port", 465),
            smtp_security: str_field("smtp_sec"),
            smtp_username: smtp_user,
            auth_vault_key: String::new(), // replaced below
            check_interval_secs: 300,
        };
        // Re-saving an existing email updates it (also migrates its secrets
        // into the current keyring backend); otherwise a fresh row is created.
        let id = match accounts::list(db)
            .unwrap_or_default()
            .into_iter()
            .find(|a| a.email_address == email)
        {
            Some(existing) => {
                if let Err(e) = accounts::update_connection(db, existing.id, &form_account) {
                    return qstring(&e.to_string());
                }
                // Host/user/password may have changed: drop the pooled
                // session so the next action connects with the new values.
                evict_imap_session(existing.id);
                // Blank password on an edit = keep the stored secret; the
                // dialog never shows it, so re-typing must not be required.
                if !password.is_empty() {
                    if let Err(e) = auth::save_account_secrets(
                        &existing.auth_vault_key,
                        &password,
                        &str_field("smtp_password"),
                    ) {
                        return qstring(&format!("{e}"));
                    }
                }
                existing.id
            }
            None => {
                if password.is_empty() {
                    return qstring("a password is required for a new account");
                }
                let vault = auth::new_vault_key();
                if let Err(e) =
                    auth::save_account_secrets(&vault, &password, &str_field("smtp_password"))
                {
                    return qstring(&format!("{e}"));
                }
                let mut with_vault = form_account;
                with_vault.auth_vault_key = vault;
                match accounts::create(db, &with_vault) {
                    Ok(id) => id,
                    Err(e) => return qstring(&e.to_string()),
                }
            }
        };
        push_feeds(&mut self, db, id, -1);
        if let Err(e) = settings::set_last_active_account_id(db, id) {
            return qstring(&e.to_string());
        }
        self.as_mut()
            .set_account_count(accounts::list(db).map(|l| l.len() as i32).unwrap_or(1));
        qstring("")
    }

    pub fn select_account(mut self: Pin<&mut Self>, id: i64) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let Ok(acc) = accounts::get(db, id) else {
            return qstring("unknown account");
        };
        // Prefer the inbox of the account we switch to.
        let folder_id = folders::list_by_account(db, acc.id)
            .unwrap_or_default()
            .into_iter()
            .find(|f| f.role == mailcore::models::FolderRole::Inbox)
            .map(|f| f.id)
            .unwrap_or(-1);
        self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
        push_feeds(&mut self, db, acc.id, folder_id);
        if let Err(e) = settings::set_last_active_account_id(db, acc.id) {
            return qstring(&e.to_string());
        }
        qstring("")
    }

    pub fn delete_account(mut self: Pin<&mut Self>, id: i64) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let Ok(acc) = accounts::get(db, id) else {
            return qstring("unknown account");
        };
        // Drop the keyring entry first: if the row went away and this failed,
        // the secret would be orphaned with nothing left pointing at it.
        if let Err(e) = auth::delete_account_secrets(&acc.auth_vault_key) {
            log::warn!("keyring entry for {} not removed: {e}", acc.email_address);
        }
        if let Err(e) = accounts::delete(db, id) {
            return qstring(&e.to_string());
        }
        // The account is gone: don't keep a live session for it.
        evict_imap_session(id);
        // Fall back to whichever account remains, if any.
        match accounts::list(db).unwrap_or_default().first() {
            Some(next) => {
                let folder_id = folders::list_by_account(db, next.id)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|f| f.role == mailcore::models::FolderRole::Inbox)
                    .map(|f| f.id)
                    .unwrap_or(-1);
                let next_id = next.id;
                self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
                push_feeds(&mut self, db, next_id, folder_id);
                let _ = settings::set_last_active_account_id(db, next_id);
            }
            None => {
                push_feeds(&mut self, db, -1, -1);
                self.as_mut().set_current_account_email(qstring(""));
                let _ = settings::set_last_active_account_id(db, 0);
            }
        }
        self.as_mut()
            .set_account_count(accounts::list(db).map(|l| l.len() as i32).unwrap_or(0));
        qstring("")
    }

    pub fn account_form(&self, id: i64) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("{}");
        };
        let Ok(a) = accounts::get(db, id) else {
            return qstring("{}");
        };
        // Passwords stay in the keyring; the dialog leaves the field blank and
        // an empty password on save keeps the stored one.
        let form = serde_json::json!({
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
        });
        qstring(&form.to_string())
    }
}
