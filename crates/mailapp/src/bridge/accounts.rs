use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::store::account_form::{self, KeyringStore};
use mailcore::store::{accounts, folders, settings};

use crate::bridge::qobject;
use crate::bridge::worker::BUSY_MESSAGE;
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
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        // Validation, the edit-or-create decision and the keyring write live
        // in mailcore, shared with the Flutter frontend. This only refreshes
        // the window afterwards.
        let id = match account_form::save(db, &form.to_string(), &mut KeyringStore) {
            Ok(id) => id,
            Err(e) => return qstring(&account_form::user_message(&e)),
        };
        push_feeds(&mut self, db, id, -1);
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

    /// Take a queued `mailapp --open` jump request (widget/notification
    /// click): returns `"<account_id>\n<folder>"` (folder may be empty =
    /// inbox), or `""` when nothing is queued. Take-once by design — each
    /// click jumps exactly once, polled by the GUI startup and timer.
    pub fn consume_pending_open(self: Pin<&mut Self>) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(_) => return qstring(""),
        };
        match settings::take_pending_open(db) {
            Some((id, folder)) => qstring(&format!("{id}\n{folder}")),
            None => qstring(""),
        }
    }

    pub fn delete_account(mut self: Pin<&mut Self>, id: i64) -> QString {
        // A queued or running job holds its account's id; deleting under it
        // would cascade away its outbox row or strand a draft mid-save.
        if *self.busy() {
            return qstring(BUSY_MESSAGE);
        }
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let Ok(acc) = accounts::get(db, id) else {
            return qstring("unknown account");
        };
        // Drop the keyring entry first: if the row went away and this failed,
        // the secret would be orphaned with nothing left pointing at it.
        if let Err(e) = mailcore::auth::delete_account_secrets(&acc.auth_vault_key) {
            log::warn!("keyring entry for {} not removed: {e}", acc.email_address);
        }
        if let Err(e) = accounts::delete(db, id) {
            return qstring(&e.to_string());
        }
        // The account is gone: don't keep a live session for it.
        mailcore::sync::pool::evict_session(id);
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
        let form = shared_db()
            .ok()
            .and_then(|db| account_form::load(db, id).ok())
            .unwrap_or_else(|| "{}".to_string());
        qstring(&form)
    }

    pub fn account_form_defaults(&self) -> QString {
        qstring(&account_form::defaults_json())
    }

    pub fn account_guess(&self, email: &QString) -> QString {
        let json = match account_form::guess(&email.to_string()) {
            Some(g) => serde_json::json!({
                "imap_host": g.imap_host,
                "smtp_host": g.smtp_host,
                "imap_user": g.imap_user,
            }),
            None => serde_json::json!({}),
        };
        qstring(&json.to_string())
    }

    pub fn account_port_for_security(
        &self,
        protocol: &QString,
        old_sec: &QString,
        new_sec: &QString,
        port: &QString,
    ) -> QString {
        let Some(protocol) = account_form::Protocol::parse(&protocol.to_string()) else {
            return port.clone();
        };
        qstring(&account_form::port_after_security_change(
            protocol,
            &old_sec.to_string(),
            &new_sec.to_string(),
            &port.to_string(),
        ))
    }

    pub fn account_form_check(&self, form: &QString, editing: bool) -> QString {
        let v = serde_json::from_str(&form.to_string()).unwrap_or_default();
        qstring(&account_form::check(&v, editing).to_json())
    }
}
