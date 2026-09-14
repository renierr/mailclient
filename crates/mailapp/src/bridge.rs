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
        #[qproperty(QString, current_account_from_name)]
        #[qproperty(QString, accounts_json)]
        #[qproperty(i32, message_limit)]
        #[qproperty(i32, messages_total)]
        #[qproperty(i32, messages_server_total)]
        #[qproperty(QString, sort_field)]
        #[qproperty(bool, sort_descending)]
        #[qproperty(bool, busy)]
        #[namespace = "mailclient"]
        type Bridge = super::BridgeRust;

        /// Fired when a background network job finishes. Feeds are already
        /// refreshed. `kind` is `"Sync"` / `"Send"` / `"Delete"` / …
        #[qsignal]
        fn job_finished(self: Pin<&mut Self>, kind: &QString, status: &QString);

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
        /// (`{name,email,from_name?,imap_host,imap_port,imap_sec,imap_user,password,
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

        /// Known sent-mail recipients as JSON, ranked by use. `prefix` matches
        /// an address or name; an empty prefix lists all contacts.
        #[qinvokable]
        fn contacts_json(&self, prefix: &QString) -> QString;

        /// Set or update a contact's custom alias. Returns `""` or an error.
        #[qinvokable]
        fn update_contact_alias(&self, address: &QString, alias: &QString) -> QString;

        /// Remove one auto-collected recipient. Returns `""` or an error.
        #[qinvokable]
        fn delete_contact(&self, address: &QString) -> QString;

        /// Run a full IMAP sync for the current account (blocking).
        /// Selective + windowed: the folder LIST is always cheap, INBOX syncs
        /// the newest 200 mails fully, every other folder only refreshes flags
        /// + the newest 50 (sidebar pills stay fresh without downloading
        /// everything). Open a folder to fetch its newest 200 on demand via
        /// `sync_folder_now`. Returns a summary or an error message.
        #[qinvokable]
        fn sync_now(self: Pin<&mut Self>) -> QString;

        /// Sync one folder (by path) fully (newest 200) on demand.
        /// Not called from the folder-click path: SELECT + UID SEARCH + body
        /// fetches are synchronous today, so that path is cache-only.
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

        /// Full reader payload for one selected message. Mailbox navigation
        /// uses compact rows so it never processes 200 message bodies at once.
        #[qinvokable]
        fn message_json(&self, uid: i32) -> QString;

        /// Attachment metadata for one message in the current folder as JSON
        /// (`[{id, filename, mime_type, size, content_id, is_inline}]`, no
        /// bytes). Mirrors the `attachments` array already in the feed; use
        /// this when the feed row is stale. Returns `"[]"` if unknown.
        #[qinvokable]
        fn attachments_json(&self, uid: i32) -> QString;

        /// Header details for one message in the current folder as JSON
        /// (`{from, to, cc, date, subject, message_id, reply_to}`) for the
        /// reader's Headers dialog. Returns `"{}"` if unknown.
        #[qinvokable]
        fn message_headers_json(&self, uid: i32) -> QString;

        /// Copy one attachment to a temp file and return its `file://` URL
        /// so QML can open it with the system viewer (`Qt.openUrlExternally`).
        /// Downloads the bytes first when they are not cached yet (explicit
        /// user request — background sync stores names/sizes only). Returns
        /// an error message instead of a URL on failure.
        #[qinvokable]
        fn open_attachment(self: Pin<&mut Self>, attachment_id: i32) -> QString;

        /// Write one attachment's bytes to `path` (plain path or `file://`
        /// URL from a save dialog). A directory target appends the attachment
        /// filename automatically. Returns `"Saved to <path>"` or an error.
        #[qinvokable]
        fn save_attachment(self: Pin<&mut Self>, attachment_id: i32, path: &QString) -> QString;

        /// Write every non-inline attachment of a message into `dir`.
        /// Returns e.g. `"Saved 3 attachments"` or an error message.
        #[qinvokable]
        fn save_all_attachments(self: Pin<&mut Self>, uid: i32, dir: &QString) -> QString;

        /// Select a folder by path and refresh the message feed.
        #[qinvokable]
        fn select_folder(self: Pin<&mut Self>, path: &QString) -> QString;

        /// Mark a message read locally. Never touches the network: the flag
        /// is queued (`flags_dirty`) and pushed by the next sync, so clicking
        /// a message cannot block on IMAP.
        #[qinvokable]
        fn open_message(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Explicitly set the read flag locally (manual mark read/unread,
        /// e.g. from the list context menu). Queued like `open_message`.
        #[qinvokable]
        fn mark_read(self: Pin<&mut Self>, uid: i32, read: bool) -> QString;

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

        /// Message-list ordering (`field` = `date`|`from`|`subject`, resilient;
        /// `descending` = newest/Z-A first). Persisted to the settings store
        /// and applied to the feed on return. Returns `""` or an error.
        #[qinvokable]
        fn set_sort(self: Pin<&mut Self>, field: &QString, descending: bool) -> QString;

        /// Bulk mark read/unread for `uids_json` (JSON array of UIDs in the
        /// current folder). Local-only, queued like `mark_read`. Returns e.g.
        /// `"Marked 5 as read"` or an error (`"no messages selected"` when empty).
        #[qinvokable]
        fn mark_read_many(self: Pin<&mut Self>, uids_json: &QString, read: bool) -> QString;

        /// Bulk star/unstar for `uids_json` (JSON array of UIDs). Local-only,
        /// queued. Returns e.g. `"Starred 5"` or an error.
        #[qinvokable]
        fn set_star_many(self: Pin<&mut Self>, uids_json: &QString, starred: bool) -> QString;

        /// Bulk delete (Trash semantics per folder, like `delete_message` but
        /// one IMAP session for the whole set). Returns e.g. `"Moved 5 to
        /// Trash"` or `"Deleted 5 permanently"`.
        #[qinvokable]
        fn delete_many(self: Pin<&mut Self>, uids_json: &QString) -> QString;

        /// Bulk archive to the Archive folder (created on demand). Returns
        /// e.g. `"Archived 5"` or `"Already in Archive"`.
        #[qinvokable]
        fn archive_many(self: Pin<&mut Self>, uids_json: &QString) -> QString;

        /// Bulk move to any same-account folder (one IMAP session). Returns
        /// e.g. `"Moved 5 to <folder>"` or `"Already here"`.
        #[qinvokable]
        fn move_many(self: Pin<&mut Self>, uids_json: &QString, path: &QString) -> QString;

        /// Bulk permanent destroy (`\Deleted` + expunge, one IMAP session).
        /// Returns e.g. `"Deleted 5 permanently"`.
        #[qinvokable]
        fn purge_many(self: Pin<&mut Self>, uids_json: &QString) -> QString;

        /// Send a message from a JSON form
        /// (`{from,from_name?,to,cc?,bcc?,subject,body,body_html?,attachments?}`; `body`
        /// holds composer rich HTML source, `body_html` is an optional
        /// explicit override, `attachments` an optional list of local file
        /// paths / `file://` URLs from the composer FileDialog).
        /// The effective MIME shape comes from the `compose_send_format`
        /// setting (`auto`|`plain`|`multipart`|`html`, resilient default
        /// `auto`) plus `compose_include_plain`. Interactive user action =
        /// explicit send consent.
        #[qinvokable]
        fn send_mail(self: Pin<&mut Self>, form: &QString) -> QString;

        /// Append or replace an IMAP `\Draft` message from a Composer form.
        #[qinvokable]
        fn save_draft(self: Pin<&mut Self>, form: &QString) -> QString;

        /// Full Composer form for a draft in the current Drafts folder.
        /// Opening a draft explicitly downloads and materializes its files.
        #[qinvokable]
        fn draft_form(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Drop all pooled IMAP sessions (app quit). No LOGOUT round-trip,
        /// so quit never blocks on a dead connection — closing the sockets
        /// reaps the server-side sessions, like any network drop.
        #[qinvokable]
        fn disconnect_all(&self);
    }

    extern "RustQt" {
        /// User preferences, persisted in SQLite via `mailcore`.
        #[qobject]
        #[qml_element]
        #[qproperty(bool, sent_copy_enabled)]
        #[qproperty(bool, load_remote_images)]
        #[qproperty(QString, compose_send_format)]
        #[qproperty(bool, compose_include_plain)]
        #[qproperty(bool, auto_mark_read)]
        #[qproperty(i32, mark_read_delay_secs)]
        #[qproperty(bool, collect_sent_contacts)]
        #[qproperty(bool, confirm_delete)]
        #[qproperty(QString, list_density)]
        #[qproperty(QString, reader_font_size)]
        #[qproperty(i32, sync_interval_minutes)]
        #[qproperty(bool, signature_enabled)]
        #[qproperty(QString, signature_text)]
        #[qproperty(bool, reply_below_quote)]
        #[qproperty(bool, request_mdn)]
        #[qproperty(f32, ui_scale)]
        #[namespace = "mailclient"]
        type SettingsBridge = super::SettingsBridgeRust;

        /// Reload properties from the settings store.
        #[qinvokable]
        fn load(self: Pin<&mut Self>);

        /// Persist current properties to the settings store.
        #[qinvokable]
        fn save(self: Pin<&mut Self>);
    }

    impl cxx_qt::Threading for Bridge {}
}

use core::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::feed;
use mailcore::store;

pub(crate) fn qstring(s: &str) -> QString {
    QString::from(s)
}

pub(crate) fn open_db() -> Result<mailcore::Db, String> {
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
    current_account_from_name: QString,
    accounts_json: QString,
    message_limit: i32,
    messages_total: i32,
    messages_server_total: i32,
    sort_field: QString,
    sort_descending: bool,
    busy: bool,
}

/// Initial older-load batch size; cached messages are always rendered in full.
pub(crate) const DEFAULT_MESSAGE_LIMIT: i32 = 200;
/// Hard cap for bulk operations and the legacy paging property.
pub(crate) const MAX_MESSAGE_LIMIT: i32 = 2000;

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
            current_account_from_name: qstring(""),
            accounts_json: qstring("[]"),
            message_limit: DEFAULT_MESSAGE_LIMIT,
            messages_total: 0,
            messages_server_total: -1,
            sort_field: qstring("date"),
            sort_descending: true,
            busy: false,
        }
    }
}

/// Push fresh JSON feeds for `(account_id, folder_id)` into the properties.
///
/// The message feed always contains the full local cache. `messages_total`
/// reports the cached DB total; `messages_server_total` is the latest count
/// reported by IMAP so QML can distinguish an incomplete cache from a fully
/// downloaded folder. The feed ordering comes
/// from the `message_sort_*` settings; the matching `sort_field` /
/// `sort_descending` properties are refreshed here too so QML sort controls
/// always show what the feed actually used.
pub(crate) fn push_feeds(
    bridge: &mut Pin<&mut qobject::Bridge>,
    db: &mailcore::Db,
    account_id: i64,
    folder_id: i64,
) {
    let folders = feed::folders_json(db, account_id).unwrap_or_else(|_| "[]".to_string());
    let (msgs, total, server_total) = if folder_id >= 0 {
        let total = store::messages::count_by_folder(db, folder_id).unwrap_or(0) as i32;
        let server_total = store::folders::get(db, folder_id)
            .ok()
            .and_then(|folder| folder.server_total)
            .map(|s| s.min(i32::MAX as u64) as i32)
            .unwrap_or(-1);
        let msgs = feed::messages_list_json_paged(db, folder_id, total as u64, 0)
            .unwrap_or_else(|_| "[]".to_string());
        (msgs, total, server_total)
    } else {
        ("[]".to_string(), 0, -1)
    };
    let email = store::accounts::get(db, account_id)
        .map(|a| a.email_address)
        .unwrap_or_default();
    let from_name = store::accounts::get(db, account_id)
        .map(|a| a.from_name)
        .unwrap_or_default();
    bridge.as_mut().set_folders_json(qstring(&folders));
    bridge.as_mut().set_messages_json(qstring(&msgs));
    bridge.as_mut().set_messages_total(total);
    bridge.as_mut().set_messages_server_total(server_total);
    bridge.as_mut().set_current_account_id(account_id);
    bridge.as_mut().set_current_folder_id(folder_id);
    bridge.as_mut().set_current_account_email(qstring(&email));
    bridge
        .as_mut()
        .set_current_account_from_name(qstring(&from_name));
    let accts = feed::accounts_json(db).unwrap_or_else(|_| "[]".to_string());
    bridge.as_mut().set_accounts_json(qstring(&accts));
    // Keep the QML-bound sort state aligned with what the feed just used.
    sync_sort_props(bridge, db);
}

/// Mirror the persisted `message_sort_*` settings into the QML-bindable
/// `sort_field` / `sort_descending` properties.
pub(crate) fn sync_sort_props(bridge: &mut Pin<&mut qobject::Bridge>, db: &mailcore::Db) {
    bridge
        .as_mut()
        .set_sort_field(qstring(&mailcore::store::settings::get_sort_field(db)));
    bridge
        .as_mut()
        .set_sort_descending(mailcore::store::settings::get_sort_descending(db));
}

impl qobject::Bridge {
    pub fn contacts_json(&self, prefix: &QString) -> QString {
        let result = open_db().and_then(|db| {
            let prefix = prefix.to_string();
            let contacts = if prefix.trim().is_empty() {
                mailcore::store::contacts::list(&db, 200)
            } else {
                mailcore::store::contacts::suggest(&db, &prefix, 10)
            };
            contacts
                .map(|contacts| {
                    serde_json::to_string(&contacts).unwrap_or_else(|_| "[]".to_string())
                })
                .map_err(|e| e.to_string())
        });
        qstring(&result.unwrap_or_else(|e| {
            log::warn!("contacts: cannot load suggestions: {e}");
            "[]".to_string()
        }))
    }

    pub fn update_contact_alias(&self, address: &QString, alias: &QString) -> QString {
        let address = address.to_string();
        let alias = alias.to_string();
        let alias_opt = if alias.trim().is_empty() {
            None
        } else {
            Some(alias.as_str())
        };
        let result = open_db().and_then(|db| {
            mailcore::store::contacts::set_alias(&db, address.trim(), alias_opt)
                .map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => qstring(""),
            Err(e) => qstring(&e),
        }
    }

    pub fn delete_contact(&self, address: &QString) -> QString {
        let address = address.to_string();
        let result = open_db().and_then(|db| {
            mailcore::store::contacts::delete(&db, address.trim()).map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => qstring(""),
            Err(e) => qstring(&e),
        }
    }

    /// Health check callable from QML.
    pub fn ping(&self, message: &QString) -> QString {
        let text = message.to_string();
        QString::from(format!("pong: {text}").as_str())
    }

    /// See [`crate::platform::set_dark_decorations`].
    pub fn apply_native_theme(&self, dark: bool) {
        crate::platform::set_dark_decorations(dark);
    }
}

/// Backing Rust struct for the `SettingsBridge` QObject.
pub struct SettingsBridgeRust {
    sent_copy_enabled: bool,
    load_remote_images: bool,
    compose_send_format: QString,
    compose_include_plain: bool,
    auto_mark_read: bool,
    mark_read_delay_secs: i32,
    collect_sent_contacts: bool,
    confirm_delete: bool,
    list_density: QString,
    reader_font_size: QString,
    sync_interval_minutes: i32,
    signature_enabled: bool,
    signature_text: QString,
    reply_below_quote: bool,
    request_mdn: bool,
    ui_scale: f32,
}

impl Default for SettingsBridgeRust {
    fn default() -> Self {
        Self {
            sent_copy_enabled: true,
            load_remote_images: false,
            compose_send_format: qstring("auto"),
            compose_include_plain: true,
            auto_mark_read: true,
            mark_read_delay_secs: 0,
            collect_sent_contacts: true,
            confirm_delete: true,
            list_density: qstring("comfortable"),
            reader_font_size: qstring("normal"),
            sync_interval_minutes: 0,
            signature_enabled: false,
            signature_text: qstring(""),
            reply_below_quote: false,
            request_mdn: false,
            ui_scale: 1.0,
        }
    }
}

mod accounts;
mod composer;
mod messages;
mod session;
mod settings;
mod sync;
mod worker;
