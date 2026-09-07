//! cxx-qt bridge: QObjects implemented in Rust, exposed to QML.
//!
//! - [`qobject::Bridge`]: app controller — accounts, folders/messages JSON
//!   feeds, sync, send. All fallible actions return a `QString` status
//!   (`""` = ok, otherwise human-readable error for the status bar).
//! - [`qobject::SettingsBridge`]: user preferences backed by the
//!   `mailcore` settings store, editable from the Settings dialog.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// Qt QString.
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        /// App-level controller object.
        #[qobject]
        #[qproperty(QString, db_path)]
        #[qproperty(i32, account_count)]
        #[qproperty(QString, folders_json)]
        #[qproperty(QString, messages_json)]
        #[qproperty(i64, current_account_id)]
        #[qproperty(i64, current_folder_id)]
        #[namespace = "mailclient"]
        type Bridge = super::BridgeRust;

        /// Health check callable from QML: returns `"pong: <message>"`.
        #[qinvokable]
        fn ping(&self, message: &QString) -> QString;

        /// Reload accounts/feeds from the DB. Returns `""` or a status message
        /// (`"no account — add one first"` when empty).
        #[qinvokable]
        fn refresh_accounts(self: Pin<&mut Self>) -> QString;

        /// Create an account from a JSON form
        /// (`{name,email,imap_host,imap_port,imap_sec,imap_user,password,
        /// smtp_host,smtp_port,smtp_sec,smtp_user}`); password goes to the OS
        /// keyring. Returns `""` or an error message.
        #[qinvokable]
        fn add_account(self: Pin<&mut Self>, form: &QString) -> QString;

        /// Run a full IMAP sync for the current account (blocking).
        /// Returns a summary or an error message.
        #[qinvokable]
        fn sync_now(self: Pin<&mut Self>) -> QString;

        /// Select a folder by path and refresh the message feed.
        #[qinvokable]
        fn select_folder(self: Pin<&mut Self>, path: &QString) -> QString;

        /// Mark a message read (locally + server flag push, best effort).
        #[qinvokable]
        fn open_message(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Flip the starred flag (locally + server flag push, best effort).
        #[qinvokable]
        fn toggle_star(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Delete a message server-side (`\Deleted` + expunge) and locally.
        #[qinvokable]
        fn delete_message(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Send a message from a JSON form (`{to,subject,body}`) via the
        /// current account. Interactive user action = explicit send consent.
        #[qinvokable]
        fn send_mail(self: Pin<&mut Self>, form: &QString) -> QString;
    }

    extern "RustQt" {
        /// User preferences, persisted in SQLite via `mailcore`.
        #[qobject]
        #[qml_element]
        #[qproperty(bool, sent_copy_enabled)]
        #[qproperty(bool, load_remote_images)]
        #[namespace = "mailclient"]
        type SettingsBridge = super::SettingsBridgeRust;

        /// Reload properties from the settings store.
        #[qinvokable]
        fn load(self: Pin<&mut Self>);

        /// Persist current properties to the settings store.
        #[qinvokable]
        fn save(self: Pin<&mut Self>);
    }
}

use core::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::models::NewAccount;
use mailcore::store::{accounts, folders, messages};
use mailcore::sync::imap::ImapSync;
use mailcore::sync::sender::{SendPolicy, SendRequest, SmtpSender};
use mailcore::sync::traits::{MailSender, SyncProvider};
use mailcore::{auth, feed};

fn qstring(s: &str) -> QString {
    QString::from(s)
}

fn open_db() -> Result<mailcore::Db, String> {
    mailcore::Db::open(&mailcore::default_db_path()).map_err(|e| e.to_string())
}

/// Backing Rust struct for the `Bridge` QObject.
pub struct BridgeRust {
    db_path: QString,
    account_count: i32,
    folders_json: QString,
    messages_json: QString,
    current_account_id: i64,
    current_folder_id: i64,
}

impl Default for BridgeRust {
    fn default() -> Self {
        Self {
            db_path: QString::from(mailcore::default_db_path().to_string_lossy().as_ref()),
            account_count: 0,
            folders_json: qstring("[]"),
            messages_json: qstring("[]"),
            current_account_id: -1,
            current_folder_id: -1,
        }
    }
}

/// Push fresh JSON feeds for `(account_id, folder_id)` into the properties.
fn push_feeds(
    bridge: &mut Pin<&mut qobject::Bridge>,
    db: &mailcore::Db,
    account_id: i64,
    folder_id: i64,
) {
    let folders = feed::folders_json(db, account_id).unwrap_or_else(|_| "[]".to_string());
    let msgs = if folder_id >= 0 {
        feed::messages_json(db, folder_id).unwrap_or_else(|_| "[]".to_string())
    } else {
        "[]".to_string()
    };
    bridge.as_mut().set_folders_json(qstring(&folders));
    bridge.as_mut().set_messages_json(qstring(&msgs));
    bridge.as_mut().set_current_account_id(account_id);
    bridge.as_mut().set_current_folder_id(folder_id);
}

/// Resolve the current account: stored id if still present, else the first.
fn current_account(db: &mailcore::Db, wanted: i64) -> Result<mailcore::models::Account, String> {
    if wanted >= 0 {
        if let Ok(a) = accounts::get(db, wanted) {
            return Ok(a);
        }
    }
    accounts::list(db)
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "no account — add one first".to_string())
}

/// Connect an IMAP session using the keyring secret.
fn imap_session(account: &mailcore::models::Account) -> Result<ImapSync, String> {
    let secret = auth::load_secret(&account.auth_vault_key)
        .map_err(|e| format!("no password in keyring: {e}"))?;
    let mut imap = ImapSync::new(account);
    imap.connect(&secret).map_err(|e| e.to_string())?;
    Ok(imap)
}

impl qobject::Bridge {
    /// Health check callable from QML.
    pub fn ping(&self, message: &QString) -> QString {
        let text = message.to_string();
        QString::from(format!("pong: {text}").as_str())
    }

    pub fn refresh_accounts(mut self: Pin<&mut Self>) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let list = match accounts::list(&db) {
            Ok(l) => l,
            Err(e) => return qstring(&e.to_string()),
        };
        self.as_mut().set_account_count(list.len() as i32);
        let Some(acc) = ({
            let wanted = *self.current_account_id();
            list.into_iter().find(|a| a.id == wanted)
        })
        .or_else(|| accounts::list(&db).ok().and_then(|l| l.into_iter().next())) else {
            push_feeds(&mut self, &db, -1, -1);
            return qstring("no account — add one first");
        };
        // Prefer inbox, else first folder.
        let folder_id = folders::list_by_account(&db, acc.id)
            .ok()
            .and_then(|fs| {
                fs.iter()
                    .find(|f| f.role == mailcore::models::FolderRole::Inbox)
                    .or_else(|| fs.first())
                    .map(|f| f.id)
            })
            .unwrap_or(-1);
        push_feeds(&mut self, &db, acc.id, folder_id);
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
        if email.is_empty() || imap_host.is_empty() || password.is_empty() {
            return qstring("fill email, IMAP host and password");
        }
        if smtp_host.is_empty() {
            return qstring("fill the SMTP host");
        }
        if smtp_user.is_empty() {
            smtp_user.clone_from(&imap_user);
        }
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        if let Ok(list) = accounts::list(&db) {
            if list.iter().any(|a| a.email_address == email) {
                return qstring("account already exists");
            }
        }
        let vault = auth::new_vault_key();
        if let Err(e) = auth::save_secret(&vault, &password) {
            return qstring(&format!("keyring unavailable: {e}"));
        }
        let id = match accounts::create(
            &db,
            &NewAccount {
                name: if name.is_empty() { email.clone() } else { name },
                email_address: email,
                imap_host,
                imap_port: u16_field("imap_port", 993),
                imap_security: str_field("imap_sec"),
                imap_username: imap_user,
                smtp_host,
                smtp_port: u16_field("smtp_port", 465),
                smtp_security: str_field("smtp_sec"),
                smtp_username: smtp_user,
                auth_vault_key: vault,
                check_interval_secs: 300,
            },
        ) {
            Ok(id) => id,
            Err(e) => return qstring(&e.to_string()),
        };
        push_feeds(&mut self, &db, id, -1);
        self.as_mut()
            .set_account_count(accounts::list(&db).map(|l| l.len() as i32).unwrap_or(1));
        qstring("")
    }

    pub fn sync_now(mut self: Pin<&mut Self>) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let result: Result<String, String> = (|| {
            let acc = current_account(&db, wanted)?;
            let mut imap = imap_session(&acc)?;
            let folders = imap.sync_folders(&db, acc.id).map_err(|e| e.to_string())?;
            let mut fetched = 0u64;
            let mut expunged = 0u64;
            for f in &folders {
                let r = imap.sync_folder(&db, f.id).map_err(|e| e.to_string())?;
                fetched += r.fetched;
                expunged += r.expunged;
            }
            imap.disconnect();
            // Keep selection if it still exists, else inbox, else first.
            let all = folders::list_by_account(&db, acc.id).map_err(|e| e.to_string())?;
            let current = *self.current_folder_id();
            let folder_id = all
                .iter()
                .find(|f| f.id == current)
                .or_else(|| {
                    all.iter()
                        .find(|f| f.role == mailcore::models::FolderRole::Inbox)
                })
                .or(all.first())
                .map(|f| f.id)
                .unwrap_or(-1);
            push_feeds(&mut self, &db, acc.id, folder_id);
            Ok(format!(
                "Synced {} folders: +{fetched} new, -{expunged} removed",
                folders.len()
            ))
        })();
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn select_folder(mut self: Pin<&mut Self>, path: &QString) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let acc = match current_account(&db, wanted) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        match folders::get_by_path(&db, acc.id, &path.to_string()) {
            Ok(f) => {
                push_feeds(&mut self, &db, acc.id, f.id);
                qstring("")
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn open_message(mut self: Pin<&mut Self>, uid: i32) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let Ok(msg) = messages::get_by_uid(&db, folder_id, uid as u32) else {
            return qstring("");
        };
        if !msg.is_read {
            let _ = messages::set_flags(&db, msg.id, true, msg.is_starred);
            // Best-effort server flag push.
            if let Ok(acc) = accounts::get(&db, acc_id) {
                if let Ok(mut imap) = imap_session(&acc) {
                    let updated = messages::get(&db, msg.id).unwrap_or(msg);
                    let _ = imap.push_flags(&db, &updated);
                    imap.disconnect();
                }
            }
        }
        push_feeds(&mut self, &db, acc_id, folder_id);
        qstring("")
    }

    pub fn toggle_star(mut self: Pin<&mut Self>, uid: i32) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let Ok(msg) = messages::get_by_uid(&db, folder_id, uid as u32) else {
            return qstring("");
        };
        let _ = messages::set_flags(&db, msg.id, msg.is_read, !msg.is_starred);
        if let Ok(acc) = accounts::get(&db, acc_id) {
            if let Ok(mut imap) = imap_session(&acc) {
                if let Ok(updated) = messages::get(&db, msg.id) {
                    let _ = imap.push_flags(&db, &updated);
                }
                imap.disconnect();
            }
        }
        push_feeds(&mut self, &db, acc_id, folder_id);
        qstring("")
    }

    pub fn delete_message(mut self: Pin<&mut Self>, uid: i32) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let Ok(msg) = messages::get_by_uid(&db, folder_id, uid as u32) else {
            return qstring("");
        };
        let acc = match accounts::get(&db, acc_id) {
            Ok(a) => a,
            Err(e) => return qstring(&e.to_string()),
        };
        let mut imap = match imap_session(&acc) {
            Ok(s) => s,
            Err(e) => return qstring(&e),
        };
        let r = imap.delete_message(&db, msg.id).map_err(|e| e.to_string());
        imap.disconnect();
        if let Err(e) = r {
            return qstring(&e);
        }
        push_feeds(&mut self, &db, acc_id, folder_id);
        qstring("")
    }

    pub fn send_mail(mut self: Pin<&mut Self>, form: &QString) -> QString {
        let v: serde_json::Value = match serde_json::from_str(&form.to_string()) {
            Ok(v) => v,
            Err(_) => return qstring("invalid message form"),
        };
        let str_field = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let to_raw = str_field("to");
        let subject = str_field("subject");
        let body = str_field("body");
        let to: Vec<String> = to_raw
            .split([',', ';'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if to.is_empty() {
            return qstring("add at least one recipient");
        }
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let acc = match current_account(&db, wanted) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        let smtp_secret = match auth::load_secret(&acc.auth_vault_key) {
            Ok(s) => s,
            Err(e) => return qstring(&format!("no password in keyring: {e}")),
        };
        // Same credential usually works for IMAP; sent-copy skips itself otherwise.
        let imap_secret = auth::load_secret(&acc.auth_vault_key).ok();
        let mut sender = SmtpSender::new(&acc);
        // Interactive Send click = explicit user consent (see SendPolicy docs).
        let req = SendRequest {
            to: &to,
            subject: &subject,
            body_text: &body,
            policy: &SendPolicy::Unrestricted,
            password: &smtp_secret,
            imap_password: imap_secret.as_deref(),
        };
        match sender.send_raw(&db, acc.id, &req) {
            Ok(()) => {
                let folder_id = *self.current_folder_id();
                push_feeds(&mut self, &db, acc.id, folder_id);
                qstring("")
            }
            Err(e) => qstring(&e.to_string()),
        }
    }
}

/// Backing Rust struct for the `SettingsBridge` QObject.
pub struct SettingsBridgeRust {
    sent_copy_enabled: bool,
    load_remote_images: bool,
}

impl Default for SettingsBridgeRust {
    fn default() -> Self {
        Self {
            sent_copy_enabled: true,
            load_remote_images: false,
        }
    }
}

impl qobject::SettingsBridge {
    fn open_db() -> Option<mailcore::Db> {
        mailcore::Db::open(&mailcore::default_db_path())
            .map_err(|e| {
                log::warn!("settings: cannot open db: {e}");
            })
            .ok()
    }

    /// Reload properties from the settings store.
    pub fn load(mut self: Pin<&mut Self>) {
        if let Some(db) = Self::open_db() {
            self.as_mut().set_sent_copy_enabled(
                mailcore::store::settings::get_bool(
                    &db,
                    mailcore::store::settings::SENT_COPY_ENABLED,
                )
                .unwrap_or(true),
            );
            self.as_mut().set_load_remote_images(
                mailcore::store::settings::get_bool(
                    &db,
                    mailcore::store::settings::LOAD_REMOTE_IMAGES,
                )
                .unwrap_or(false),
            );
        }
    }

    /// Persist current properties to the settings store.
    pub fn save(self: Pin<&mut Self>) {
        if let Some(db) = Self::open_db() {
            let sent = *self.sent_copy_enabled();
            let remote = *self.load_remote_images();
            if let Err(e) = mailcore::store::settings::set_bool(
                &db,
                mailcore::store::settings::SENT_COPY_ENABLED,
                sent,
            ) {
                log::warn!("settings: cannot save sent-copy: {e}");
            }
            if let Err(e) = mailcore::store::settings::set_bool(
                &db,
                mailcore::store::settings::LOAD_REMOTE_IMAGES,
                remote,
            ) {
                log::warn!("settings: cannot save remote-images: {e}");
            }
        }
    }
}
