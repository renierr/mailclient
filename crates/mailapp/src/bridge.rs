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
        #[qml_element]
        #[qproperty(QString, db_path)]
        #[qproperty(i32, account_count)]
        #[qproperty(QString, folders_json)]
        #[qproperty(QString, messages_json)]
        #[qproperty(i64, current_account_id)]
        #[qproperty(i64, current_folder_id)]
        #[qproperty(QString, current_account_email)]
        #[qproperty(QString, accounts_json)]
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
        /// smtp_host,smtp_port,smtp_sec,smtp_user,smtp_password}`); passwords go
        /// to the OS keyring (empty SMTP password = same as IMAP).
        /// Returns `""` or an error message.
        #[qinvokable]
        fn add_account(self: Pin<&mut Self>, form: &QString) -> QString;

        /// Switch the active account and refresh the feeds.
        #[qinvokable]
        fn select_account(self: Pin<&mut Self>, id: i64) -> QString;

        /// Delete an account with its folders/messages and keyring secrets.
        /// Returns `""` or an error message.
        #[qinvokable]
        fn delete_account(self: Pin<&mut Self>, id: i64) -> QString;

        /// One account as a JSON form for the edit dialog (no password —
        /// secrets never leave the keyring). `{}` if the id is unknown.
        #[qinvokable]
        fn account_form(&self, id: i64) -> QString;

        /// Run a full IMAP sync for the current account (blocking).
        /// Returns a summary or an error message.
        #[qinvokable]
        fn sync_now(self: Pin<&mut Self>) -> QString;

        /// Select a folder by path and refresh the message feed.
        #[qinvokable]
        fn select_folder(self: Pin<&mut Self>, path: &QString) -> QString;

        /// Mark a message read locally. Never touches the network: the flag
        /// is queued (`flags_dirty`) and pushed by the next sync, so clicking
        /// a message cannot block on IMAP.
        #[qinvokable]
        fn open_message(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Flip the starred flag locally; pushed by the next sync.
        #[qinvokable]
        fn toggle_star(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Delete a message server-side (`\Deleted` + expunge) and locally.
        #[qinvokable]
        fn delete_message(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Send a message from a JSON form
        /// (`{from,to,subject,body,body_html?}`; `body` holds composer rich
        /// HTML source, `body_html` is an optional explicit override).
        /// The effective MIME shape comes from the `compose_send_format`
        /// setting (`plain`|`multipart`|`html`, resilient default
        /// `multipart`). Interactive user action = explicit send consent.
        #[qinvokable]
        fn send_mail(self: Pin<&mut Self>, form: &QString) -> QString;
    }

    extern "RustQt" {
        /// User preferences, persisted in SQLite via `mailcore`.
        #[qobject]
        #[qml_element]
        #[qproperty(bool, sent_copy_enabled)]
        #[qproperty(bool, load_remote_images)]
        #[qproperty(QString, compose_send_format)]
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
use mailcore::sync::sender::{SendFormat, SendPolicy, SendRequest, SmtpSender};
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
    current_account_email: QString,
    accounts_json: QString,
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
            current_account_email: qstring(""),
            accounts_json: qstring("[]"),
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
    let email = accounts::get(db, account_id)
        .map(|a| a.email_address)
        .unwrap_or_default();
    bridge.as_mut().set_folders_json(qstring(&folders));
    bridge.as_mut().set_messages_json(qstring(&msgs));
    bridge.as_mut().set_current_account_id(account_id);
    bridge.as_mut().set_current_folder_id(folder_id);
    bridge.as_mut().set_current_account_email(qstring(&email));
    let accts = feed::accounts_json(db).unwrap_or_else(|_| "[]".to_string());
    bridge.as_mut().set_accounts_json(qstring(&accts));
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
    let secrets = auth::load_account_secrets(&account.auth_vault_key)
        .map_err(|e| format!("no password in keyring: {e}"))?;
    let mut imap = ImapSync::new(account);
    imap.connect(&secrets.imap_password)
        .map_err(|e| e.to_string())?;
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
        if email.is_empty() || imap_host.is_empty() {
            return qstring("fill email and IMAP host");
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
        let account_name = if name.is_empty() { email.clone() } else { name };
        let form_account = NewAccount {
            name: account_name,
            email_address: email.clone(),
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
        let id = match accounts::list(&db)
            .unwrap_or_default()
            .into_iter()
            .find(|a| a.email_address == email)
        {
            Some(existing) => {
                if let Err(e) = accounts::update_connection(&db, existing.id, &form_account) {
                    return qstring(&e.to_string());
                }
                // Blank password on an edit = keep the stored secret; the
                // dialog never shows it, so re-typing must not be required.
                if !password.is_empty() {
                    if let Err(e) = auth::save_account_secrets(
                        &existing.auth_vault_key,
                        &password,
                        &str_field("smtp_password"),
                    ) {
                        return qstring(&format!("keyring unavailable: {e}"));
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
                    return qstring(&format!("keyring unavailable: {e}"));
                }
                let mut with_vault = form_account;
                with_vault.auth_vault_key = vault;
                match accounts::create(&db, &with_vault) {
                    Ok(id) => id,
                    Err(e) => return qstring(&e.to_string()),
                }
            }
        };
        push_feeds(&mut self, &db, id, -1);
        self.as_mut()
            .set_account_count(accounts::list(&db).map(|l| l.len() as i32).unwrap_or(1));
        qstring("")
    }

    pub fn select_account(mut self: Pin<&mut Self>, id: i64) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let Ok(acc) = accounts::get(&db, id) else {
            return qstring("unknown account");
        };
        // Prefer the inbox of the account we switch to.
        let folder_id = folders::list_by_account(&db, acc.id)
            .unwrap_or_default()
            .into_iter()
            .find(|f| f.role == mailcore::models::FolderRole::Inbox)
            .map(|f| f.id)
            .unwrap_or(-1);
        push_feeds(&mut self, &db, acc.id, folder_id);
        qstring("")
    }

    pub fn delete_account(mut self: Pin<&mut Self>, id: i64) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let Ok(acc) = accounts::get(&db, id) else {
            return qstring("unknown account");
        };
        // Drop the keyring entry first: if the row went away and this failed,
        // the secret would be orphaned with nothing left pointing at it.
        if let Err(e) = auth::delete_account_secrets(&acc.auth_vault_key) {
            log::warn!("keyring entry for {} not removed: {e}", acc.email_address);
        }
        if let Err(e) = accounts::delete(&db, id) {
            return qstring(&e.to_string());
        }
        // Fall back to whichever account remains, if any.
        match accounts::list(&db).unwrap_or_default().first() {
            Some(next) => {
                let folder_id = folders::list_by_account(&db, next.id)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|f| f.role == mailcore::models::FolderRole::Inbox)
                    .map(|f| f.id)
                    .unwrap_or(-1);
                let next_id = next.id;
                push_feeds(&mut self, &db, next_id, folder_id);
            }
            None => {
                push_feeds(&mut self, &db, -1, -1);
                self.as_mut().set_current_account_email(qstring(""));
            }
        }
        self.as_mut()
            .set_account_count(accounts::list(&db).map(|l| l.len() as i32).unwrap_or(0));
        qstring("")
    }

    pub fn account_form(&self, id: i64) -> QString {
        let Ok(db) = open_db() else {
            return qstring("{}");
        };
        let Ok(a) = accounts::get(&db, id) else {
            return qstring("{}");
        };
        // Passwords stay in the keyring; the dialog leaves the field blank and
        // an empty password on save keeps the stored one.
        let form = serde_json::json!({
            "id": a.id,
            "name": a.name,
            "email": a.email_address,
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

    pub fn sync_now(mut self: Pin<&mut Self>) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let result: Result<String, String> = (|| {
            let acc = current_account(&db, wanted)?;
            let mut imap = imap_session(&acc)?;
            // Push locally queued read/star changes first, so the fetch below
            // cannot overwrite them with stale server flags.
            let mut pushed = 0u64;
            for m in messages::list_flags_dirty(&db, acc.id).unwrap_or_default() {
                if imap.push_flags(&db, &m).is_ok() {
                    let _ = messages::clear_flags_dirty(&db, m.id);
                    pushed += 1;
                }
            }
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
            let flags = if pushed > 0 {
                format!(", {pushed} flag(s) pushed")
            } else {
                String::new()
            };
            Ok(format!(
                "Synced {} folders: +{fetched} new, -{expunged} removed{flags}",
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
            // Local write + dirty mark only. Pushing \Seen here meant a full
            // IMAP connect on every click, which froze the list and made
            // selection appear stuck; `sync_now` flushes the queue instead.
            let _ = messages::set_flags(&db, msg.id, true, msg.is_starred);
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
        // Queued, not pushed: see open_message.
        let _ = messages::set_flags(&db, msg.id, msg.is_read, !msg.is_starred);
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
        let from_raw = str_field("from");
        let from = if from_raw.is_empty() {
            None
        } else {
            Some(from_raw.as_str())
        };
        let subject = str_field("subject");
        let body = str_field("body");
        // Optional explicit HTML override (new Composer sends both; old
        // payloads only have `body` holding rich HTML source — handled in
        // `resolve_bodies` either way).
        let body_html_raw = str_field("body_html");
        let body_html = if body_html_raw.trim().is_empty() {
            None
        } else {
            Some(body_html_raw)
        };
        let to: Vec<String> = to_raw
            .split([',', ';'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if to.is_empty() {
            return qstring("add at least one recipient");
        }
        let cc: Vec<String> = str_field("cc")
            .split([',', ';'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let acc = match current_account(&db, wanted) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        let secrets = match auth::load_account_secrets(&acc.auth_vault_key) {
            Ok(s) => s,
            Err(e) => return qstring(&format!("no password in keyring: {e}")),
        };
        let mut sender = SmtpSender::new(&acc);
        // Interactive Send click = explicit user consent (see SendPolicy docs).
        // Resilient: unknown setting values fall back to multipart.
        let format = SendFormat::parse(&mailcore::store::settings::get_send_format(&db));
        let req = SendRequest {
            to: &to,
            cc: &cc,
            from,
            subject: &subject,
            body_text: &body,
            body_html: body_html.as_deref(),
            format,
            policy: &SendPolicy::Unrestricted,
            password: &secrets.smtp_password,
            imap_password: Some(&secrets.imap_password),
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
    compose_send_format: QString,
}

impl Default for SettingsBridgeRust {
    fn default() -> Self {
        Self {
            sent_copy_enabled: true,
            load_remote_images: false,
            compose_send_format: qstring("multipart"),
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
            self.as_mut()
                .set_compose_send_format(qstring(&mailcore::store::settings::get_send_format(&db)));
        }
    }

    /// Persist current properties to the settings store.
    pub fn save(self: Pin<&mut Self>) {
        if let Some(db) = Self::open_db() {
            let sent = *self.sent_copy_enabled();
            let remote = *self.load_remote_images();
            let format = mailcore::store::settings::normalize_send_format(
                &self.compose_send_format().to_string(),
            )
            .to_string();
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
            if let Err(e) = mailcore::store::settings::set(
                &db,
                mailcore::store::settings::COMPOSE_SEND_FORMAT,
                &format,
            ) {
                log::warn!("settings: cannot save send-format: {e}");
            }
        }
    }
}
