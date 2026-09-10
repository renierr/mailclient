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
        #[qproperty(i32, message_limit)]
        #[qproperty(i32, messages_total)]
        #[namespace = "mailclient"]
        type Bridge = super::BridgeRust;

        /// Health check callable from QML: returns `"pong: <message>"`.
        #[qinvokable]
        fn ping(&self, message: &QString) -> QString;

        /// Ask the window manager for dark or light window decorations.
        /// Windows needs telling explicitly (Qt leaves the caption bar light
        /// on a dark desktop); elsewhere this is a no-op.
        #[qinvokable]
        fn apply_native_theme(&self, dark: bool);

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
        /// Selective + windowed: the folder LIST is always cheap, INBOX syncs
        /// the newest 200 mails fully, every other folder only refreshes flags
        /// + the newest 50 (sidebar pills stay fresh without downloading
        /// everything). Open a folder to fetch its newest 200 on demand via
        /// `sync_folder_now`. Returns a summary or an error message.
        #[qinvokable]
        fn sync_now(self: Pin<&mut Self>) -> QString;

        /// Sync one folder (by path) fully (newest 200) on demand.
        /// Used when opening a folder and after sends/deletes — cheap because
        /// it is a single SELECT + windowed FETCH, not all folders.
        /// Returns a summary or an error message.
        #[qinvokable]
        fn sync_folder_now(self: Pin<&mut Self>, path: &QString) -> QString;

        /// Fetch the next older batch for the current folder (one page).
        /// Grows `message_limit` by what was fetched and refreshes the feed,
        /// so the list visibly extends backwards. Returns e.g.
        /// `"Loaded 200 older messages"` or `"Caught up — no older messages"`.
        #[qinvokable]
        fn load_older_messages(self: Pin<&mut Self>) -> QString;

        /// Refresh only the folder LIST from the server (no message bodies).
        /// This is how new/renamed IMAP folders appear: cheap, then pick what
        /// to view in the Folders manager.
        /// Returns a summary or an error message.
        #[qinvokable]
        fn refresh_folders(self: Pin<&mut Self>) -> QString;

        /// Show/hide a folder in the sidebar (`subscribed` flag, display-only).
        /// Hidden folders keep their cache, still quick-sync for pills unless
        /// skipped, and an explicit open still syncs them.
        /// Returns `""` or an error message.
        #[qinvokable]
        fn set_folder_subscribed(self: Pin<&mut Self>, path: &QString, subscribed: bool)
            -> QString;

        /// Sanitized HTML for one message in the current folder.
        /// `allow_remote=true` re-sanitizes the stored raw body with remote
        /// images kept — the "Show once" path (the list feed strips them when
        /// the setting is off, so it cannot reuse `body_html`).
        /// Returns the HTML string, or `""` if unknown.
        #[qinvokable]
        fn message_html(&self, uid: i32, allow_remote: bool) -> QString;

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

        /// Move a message to Trash (what the delete action means), except:
        /// spam is destroyed immediately (junk never touches Trash) and so is
        /// anything deleted from inside Trash itself. Returns `"Moved to
        /// <folder>"`, or `"Deleted permanently"` for both destroy cases.
        #[qinvokable]
        fn delete_message(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Move a message to the Archive folder (one-click archive).
        /// Creates the Archive folder server-side when the account has none.
        /// Returns `"Archived to <folder>"` or `"Already in Archive"`.
        #[qinvokable]
        fn archive_message(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Move a message to any folder of the same account (by path,
        /// subfolders included — hierarchy is part of the path).
        /// Returns `"Moved to <folder>"` or `"Already here"`.
        #[qinvokable]
        fn move_message(self: Pin<&mut Self>, uid: i32, path: &QString) -> QString;

        /// Create an IMAP folder (`/` separates levels, e.g. `Work/Client`;
        /// mapped onto the account's hierarchy delimiter). Missing parents
        /// are created too; an existing path is success. Returns `"Created
        /// <path>"`, `"Folder already exists"`, or an error message.
        #[qinvokable]
        fn create_folder(self: Pin<&mut Self>, path: &QString) -> QString;

        /// Destroy a message server-side (`\Deleted` + expunge). No undo;
        /// only for an explicit "delete permanently" action.
        #[qinvokable]
        fn purge_message(self: Pin<&mut Self>, uid: i32) -> QString;

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
use mailcore::sync::imap::{
    ArchiveOutcome, ImapSync, MoveOutcome, TrashOutcome, FULL_SYNC_WINDOW, OLDER_BATCH,
    QUICK_SYNC_WINDOW,
};
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
    message_limit: i32,
    messages_total: i32,
}

/// First-page size for a freshly opened folder (matches feed + sync window).
const DEFAULT_MESSAGE_LIMIT: i32 = 200;
/// Hard cap so "load older" cannot grow the JSON feed without bound.
const MAX_MESSAGE_LIMIT: i32 = 2000;

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
            message_limit: DEFAULT_MESSAGE_LIMIT,
            messages_total: 0,
        }
    }
}

/// Clamp a feed limit into the safe range.
fn clamp_limit(n: i32) -> u64 {
    (n.clamp(50, MAX_MESSAGE_LIMIT)) as u64
}

/// Push fresh JSON feeds for `(account_id, folder_id)` into the properties.
///
/// The message feed is paged by `message_limit` (grows via "load older");
/// `messages_total` reports the cached DB total so QML knows whether more
/// rows exist locally or a server backfill is needed.
fn push_feeds(
    bridge: &mut Pin<&mut qobject::Bridge>,
    db: &mailcore::Db,
    account_id: i64,
    folder_id: i64,
) {
    let folders = feed::folders_json(db, account_id).unwrap_or_else(|_| "[]".to_string());
    let limit = clamp_limit(*bridge.message_limit());
    let (msgs, total) = if folder_id >= 0 {
        let total = messages::count_by_folder(db, folder_id).unwrap_or(0) as i32;
        let msgs =
            feed::messages_json_paged(db, folder_id, limit, 0).unwrap_or_else(|_| "[]".to_string());
        (msgs, total)
    } else {
        ("[]".to_string(), 0)
    };
    let email = accounts::get(db, account_id)
        .map(|a| a.email_address)
        .unwrap_or_default();
    bridge.as_mut().set_folders_json(qstring(&folders));
    bridge.as_mut().set_messages_json(qstring(&msgs));
    bridge.as_mut().set_messages_total(total);
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

/// Run a fallible sync action, converting a Rust panic into an error string.
///
/// cxx turns any panic crossing the QML bridge into SIGABRT (its Guard
/// double-panics by design), which kills the app on something as routine as
/// startup auto-sync. A sync panic must surface as a status message instead —
/// the failure is logged with its payload for diagnosis.
fn guard_sync(label: &str, f: impl FnOnce() -> Result<String, String>) -> Result<String, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown cause".to_string());
            log::error!("{label} aborted by panic: {detail}");
            Err(format!("{label} hit an internal error ({detail})"))
        }
    }
}

impl qobject::Bridge {
    /// Health check callable from QML.
    pub fn ping(&self, message: &QString) -> QString {
        let text = message.to_string();
        QString::from(format!("pong: {text}").as_str())
    }

    /// See [`crate::platform::set_dark_decorations`].
    pub fn apply_native_theme(&self, dark: bool) {
        crate::platform::set_dark_decorations(dark);
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
        // Fresh account context: restart paging from the first page.
        self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
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
        self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
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
                self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
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
        let result = guard_sync("Sync", || {
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
            // Selective: INBOX gets the full window (newest 200 full bodies),
            // every other *visible* folder only flags + newest 50. Hidden
            // (unsubscribed) folders are LISTed so they stay manageable, but
            // their bodies are skipped — open one explicitly and it syncs.
            // Custom folders never auto-sync all mail — they fill on demand.
            let mut fetched = 0u64;
            let mut expunged = 0u64;
            let mut quick = 0usize;
            let mut skipped = 0usize;
            for f in &folders {
                if !f.subscribed {
                    skipped += 1;
                    continue;
                }
                let window = if f.role == mailcore::models::FolderRole::Inbox {
                    Some(FULL_SYNC_WINDOW)
                } else {
                    quick += 1;
                    Some(QUICK_SYNC_WINDOW)
                };
                let r = imap
                    .sync_folder_window(&db, f.id, window)
                    .map_err(|e| e.to_string())?;
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
            let scope = if quick > 0 {
                format!(" (inbox full, {quick} folder(s) quick)")
            } else {
                String::new()
            };
            let hidden = if skipped > 0 {
                format!(", {skipped} hidden skipped")
            } else {
                String::new()
            };
            Ok(format!(
                "Synced {} folders: +{fetched} new, -{expunged} removed{flags}{scope}{hidden}",
                folders.len()
            ))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn sync_folder_now(mut self: Pin<&mut Self>, path: &QString) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let result = guard_sync("Sync", || {
            let acc = current_account(&db, wanted)?;
            let folder =
                folders::get_by_path(&db, acc.id, &path.to_string()).map_err(|e| e.to_string())?;
            // Flush pending flag pushes first so this folder's fetch cannot
            // revert a just-tapped read/star.
            let mut imap = imap_session(&acc)?;
            for m in messages::list_flags_dirty(&db, acc.id).unwrap_or_default() {
                if imap.push_flags(&db, &m).is_ok() {
                    let _ = messages::clear_flags_dirty(&db, m.id);
                }
            }
            let r = imap
                .sync_folder_window(&db, folder.id, Some(FULL_SYNC_WINDOW))
                .map_err(|e| e.to_string())?;
            imap.disconnect();
            // Stay on the synced folder.
            push_feeds(&mut self, &db, acc.id, folder.id);
            Ok(format!(
                "Synced {}: +{} new, -{} removed",
                folder.path, r.fetched, r.expunged
            ))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn load_older_messages(mut self: Pin<&mut Self>) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        if folder_id < 0 {
            return qstring("no folder selected");
        }
        let result = guard_sync("Sync", || {
            let acc = current_account(&db, wanted)?;
            // Sanity: the folder must belong to this account.
            let folder = folders::get(&db, folder_id).map_err(|e| e.to_string())?;
            if folder.account_id != acc.id {
                return Err("folder does not belong to this account".to_string());
            }
            let mut imap = imap_session(&acc)?;
            let r = imap
                .sync_older(&db, folder_id, OLDER_BATCH)
                .map_err(|e| e.to_string())?;
            imap.disconnect();
            if r.fetched > 0 {
                let grown = (*self.message_limit() + r.fetched as i32).min(MAX_MESSAGE_LIMIT);
                self.as_mut().set_message_limit(grown);
            }
            push_feeds(&mut self, &db, acc.id, folder_id);
            if r.fetched > 0 {
                Ok(format!("Loaded {} older messages", r.fetched))
            } else {
                Ok("Caught up — no older messages on the server".to_string())
            }
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn refresh_folders(mut self: Pin<&mut Self>) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let result = guard_sync("Sync", || {
            let acc = current_account(&db, wanted)?;
            let mut imap = imap_session(&acc)?;
            let list = imap.sync_folders(&db, acc.id).map_err(|e| e.to_string())?;
            imap.disconnect();
            let current = *self.current_folder_id();
            let still_there = list.iter().any(|f| f.id == current);
            let folder_id = if still_there {
                current
            } else {
                folders::list_by_account(&db, acc.id)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .find(|f| f.role == mailcore::models::FolderRole::Inbox)
                    .map(|f| f.id)
                    .unwrap_or(-1)
            };
            push_feeds(&mut self, &db, acc.id, folder_id);
            Ok(format!("Found {} IMAP folders", list.len()))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn set_folder_subscribed(
        mut self: Pin<&mut Self>,
        path: &QString,
        subscribed: bool,
    ) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let acc = match current_account(&db, wanted) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        let folder = match folders::get_by_path(&db, acc.id, &path.to_string()) {
            Ok(f) => f,
            Err(e) => return qstring(&e.to_string()),
        };
        if let Err(e) = folders::set_subscribed(&db, folder.id, subscribed) {
            return qstring(&e.to_string());
        }
        let current = *self.current_folder_id();
        push_feeds(&mut self, &db, acc.id, current);
        qstring("")
    }

    pub fn message_html(&self, uid: i32, allow_remote: bool) -> QString {
        let Ok(db) = open_db() else {
            return qstring("");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("");
        }
        feed::message_html(&db, folder_id, uid as u32, allow_remote)
            .map_or_else(|_| qstring(""), |h| qstring(&h))
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
                // New folder context: restart paging from the first page.
                self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
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
        // Guarded: any panic becomes a status message, never SIGABRT.
        let result = guard_sync("Delete", || {
            let db = open_db()?;
            let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
            let msg =
                messages::get_by_uid(&db, folder_id, uid as u32).map_err(|_| String::new())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let mut imap = imap_session(&acc)?;
            let r = imap.trash_message(&db, msg.id).map_err(|e| e.to_string());
            imap.disconnect();
            let outcome = r?;
            push_feeds(&mut self, &db, acc_id, folder_id);
            // Reported, not silent: "deleted permanently" is a different promise
            // from "moved to Trash" and the user needs to know which happened.
            Ok(match outcome {
                TrashOutcome::Moved(path) => format!("Moved to {path}"),
                TrashOutcome::Expunged => "Deleted permanently".to_string(),
            })
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn archive_message(mut self: Pin<&mut Self>, uid: i32) -> QString {
        // Guarded: any panic becomes a status message, never SIGABRT.
        let result = guard_sync("Archive", || {
            let db = open_db()?;
            let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
            let msg =
                messages::get_by_uid(&db, folder_id, uid as u32).map_err(|_| String::new())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let mut imap = imap_session(&acc)?;
            let r = imap.archive_message(&db, msg.id).map_err(|e| e.to_string());
            imap.disconnect();
            let outcome = r?;
            push_feeds(&mut self, &db, acc_id, folder_id);
            Ok(match outcome {
                ArchiveOutcome::Moved(path) => format!("Archived to {path}"),
                ArchiveOutcome::AlreadyThere => "Already in Archive".to_string(),
            })
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn move_message(mut self: Pin<&mut Self>, uid: i32, path: &QString) -> QString {
        // Guarded: any panic becomes a status message, never SIGABRT.
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let path = path.to_string();
        let result = guard_sync("Move", || {
            let db = open_db()?;
            let acc = current_account(&db, wanted)?;
            let msg = messages::get_by_uid(&db, current, uid as u32).map_err(|_| String::new())?;
            let dest = folders::get_by_path(&db, acc.id, &path).map_err(|e| e.to_string())?;
            let mut imap = imap_session(&acc)?;
            let r = imap
                .move_to_folder(&db, msg.id, dest.id)
                .map_err(|e| e.to_string());
            imap.disconnect();
            let outcome = r?;
            push_feeds(&mut self, &db, acc.id, current);
            Ok(match outcome {
                MoveOutcome::Moved(path) => format!("Moved to {path}"),
                MoveOutcome::AlreadyThere => "Already here".to_string(),
            })
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn create_folder(mut self: Pin<&mut Self>, path: &QString) -> QString {
        // Guarded: any panic becomes a status message, never SIGABRT.
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let path = path.to_string();
        let result = guard_sync("Sync", || {
            let db = open_db()?;
            let acc = current_account(&db, wanted)?;
            // Already known locally (same normalized path) = success.
            let delimiter = folders::list_by_account(&db, acc.id)
                .unwrap_or_default()
                .first()
                .map(|f| f.delimiter.clone())
                .unwrap_or_else(|| "/".to_string());
            let normalized =
                mailcore::sync::imap::normalize_folder_path(&path.to_string(), &delimiter)
                    .map_err(|e| e.to_string())?;
            if folders::get_by_path(&db, acc.id, &normalized).is_ok() {
                push_feeds(&mut self, &db, acc.id, current);
                return Ok("Folder already exists".to_string());
            }
            let mut imap = imap_session(&acc)?;
            let folder = imap
                .create_folder_path(&db, acc.id, &normalized, &delimiter)
                .map_err(|e| e.to_string())?;
            imap.disconnect();
            push_feeds(&mut self, &db, acc.id, current);
            Ok(format!("Created {}", folder.path))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn purge_message(mut self: Pin<&mut Self>, uid: i32) -> QString {
        // Guarded: any panic becomes a status message, never SIGABRT.
        let result = guard_sync("Delete", || {
            let db = open_db()?;
            let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
            let msg =
                messages::get_by_uid(&db, folder_id, uid as u32).map_err(|_| String::new())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let mut imap = imap_session(&acc)?;
            let r = imap.delete_message(&db, msg.id).map_err(|e| e.to_string());
            imap.disconnect();
            r?;
            push_feeds(&mut self, &db, acc_id, folder_id);
            Ok("Deleted permanently".to_string())
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
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
        // Guarded: any panic becomes a status message, never SIGABRT.
        let result = guard_sync("Send", || {
            let db = open_db()?;
            let wanted = *self.current_account_id();
            let acc = current_account(&db, wanted)?;
            let secrets = auth::load_account_secrets(&acc.auth_vault_key)
                .map_err(|e| format!("no password in keyring: {e}"))?;
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
            sender
                .send_raw(&db, acc.id, &req)
                .map_err(|e| e.to_string())?;
            // Refresh after send: the SMTP + APPEND already happened, so
            // pull the Sent copy (if enabled) best-effort — offline or
            // server hiccup must never fail a successful send.
            if let Ok(sent) = folders::list_by_account(&db, acc.id)
                .unwrap_or_default()
                .into_iter()
                .find(|f| f.role == mailcore::models::FolderRole::Sent)
                .map(|f| f.id)
                .ok_or(())
            {
                if let Ok(mut imap) = imap_session(&acc) {
                    let _ = imap.sync_folder_window(&db, sent, Some(QUICK_SYNC_WINDOW));
                    imap.disconnect();
                }
            }
            let folder_id = *self.current_folder_id();
            push_feeds(&mut self, &db, acc.id, folder_id);
            Ok(String::new())
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
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
