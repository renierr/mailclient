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
        #[qproperty(QString, messages_older)]
        #[qproperty(bool, messages_can_load_older)]
        #[qproperty(QString, sort_field)]
        #[qproperty(bool, sort_descending)]
        #[qproperty(bool, busy)]
        #[qproperty(QString, app_version)]
        #[qproperty(QString, app_license)]
        #[namespace = "mailclient"]
        type Bridge = super::BridgeRust;

        /// Fired when a background network job finishes. Feeds are already
        /// refreshed. `kind` is `"Sync"` / `"Send"` / `"Delete"` / …,
        /// `outcome` is the machine-readable result (`SendOutcome::outcome`:
        /// `"sent"` / `"sent_partial"`, `""` for jobs without one) — key
        /// decisions off it, never off matching the status prose.
        #[qsignal]
        fn job_finished(self: Pin<&mut Self>, kind: &QString, status: &QString, outcome: &QString);

        /// Fired when a job reaches a milestone the UI should act on before
        /// the job itself is done — SMTP accepting a message, say, so the
        /// composer can close while the Sent copy and the folder refresh
        /// still run. Feeds are *not* refreshed yet; `job_finished` follows.
        #[qsignal]
        fn job_progress(self: Pin<&mut Self>, kind: &QString, status: &QString);

        /// An undoable delete/archive/move was queued: show `label` with an
        /// Undo that passes `batch` to `undo_move`, for `undo_grace_secs`.
        #[qsignal]
        fn undo_available(self: Pin<&mut Self>, batch: &QString, label: &QString);

        /// Take back a queued action before it reaches the server. Returns
        /// the status line text (also when it was too late).
        #[qinvokable]
        fn undo_move(self: Pin<&mut Self>, batch: &QString) -> QString;

        /// Seconds an action stays undoable.
        #[qinvokable]
        fn undo_grace_secs(&self) -> i32;

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

        /// Take a queued `mailapp --open` jump request: `"<id>\n<folder>"`
        /// (empty folder = inbox) or `""`. Take-once — each click jumps once.
        #[qinvokable]
        fn consume_pending_open(self: Pin<&mut Self>) -> QString;

        /// Delete an account with its folders/messages and keyring secrets.
        /// Returns `""` or an error message.
        #[qinvokable]
        fn delete_account(self: Pin<&mut Self>, id: i64) -> QString;

        /// One account as a JSON form for the edit dialog (no password —
        /// secrets never leave the keyring). `{}` if the id is unknown.
        #[qinvokable]
        fn account_form(&self, id: i64) -> QString;

        /// A new account form's starting values and security choices (JSON).
        #[qinvokable]
        fn account_form_defaults(&self) -> QString;

        /// Server guesses for a typed address as JSON
        /// (`imap_host`, `smtp_host`, `imap_user`), `{}` while it is partial.
        #[qinvokable]
        fn account_guess(&self, email: &QString) -> QString;

        /// The port field after `protocol`'s security changed `old_sec` → `new_sec`.
        #[qinvokable]
        fn account_port_for_security(
            &self,
            protocol: &QString,
            old_sec: &QString,
            new_sec: &QString,
            port: &QString,
        ) -> QString;

        /// Per-field `errors` and `warnings` for the account form (JSON).
        #[qinvokable]
        fn account_form_check(&self, form: &QString, editing: bool) -> QString;

        /// Known sent-mail recipients as JSON, ranked by use. `prefix` matches
        /// an address or name; an empty prefix lists all contacts.
        #[qinvokable]
        fn contacts_json(&self, prefix: &QString) -> QString;

        /// The recipient address currently being typed: the last `,`/`;`
        /// segment outside double quotes (`compose::recipient_segment`).
        #[qinvokable]
        fn recipient_segment(&self, text: &QString) -> QString;

        /// The field after completing its current segment with `replacement`
        /// (`compose::replace_recipient_segment`).
        #[qinvokable]
        fn replace_recipient_segment(&self, text: &QString, replacement: &QString) -> QString;

        /// Set or update a contact's custom alias. Returns `""` or an error.
        #[qinvokable]
        fn update_contact_alias(&self, address: &QString, alias: &QString) -> QString;

        /// Remove one auto-collected recipient. Returns `""` or an error.
        #[qinvokable]
        fn delete_contact(&self, address: &QString) -> QString;

        /// Contacts the cleanup review suggests removing (automated senders,
        /// long-unseen one-offs), as JSON with per-row reasons.
        #[qinvokable]
        fn cleanup_candidates_json(&self) -> QString;

        /// Remove several contacts at once (JSON array of addresses).
        /// Returns `""` or an error.
        #[qinvokable]
        fn delete_contacts(&self, addresses: &QString) -> QString;

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

        /// Painted sidebar rows (`[{id, collapsible, expanded, unread,
        /// total}]`) for `expanded_json` (a JSON array of expanded folder
        /// ids, `[]` = all collapsed). The single implementation of the
        /// sidebar fold (`mailcore::feed::sidebar_rows`); `"[]"` on error.
        #[qinvokable]
        fn sidebar_rows_json(&self, account_id: i64, expanded_json: &QString) -> QString;

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

        /// The reader's paint for one HTML mail (`mailcore::html::reader::
        /// paint_for`): `theme`, `original` or `darkened`.
        #[qinvokable]
        fn reader_paint(&self, colored: bool, dark: bool, keep_original: bool) -> QString;

        /// The colours a reader page is written in, as JSON
        /// (`{paper, ink, link, quote, rule}`), for `paint` and the theme's
        /// colours (`theme_json`, the same keys as `#rrggbb`).
        #[qinvokable]
        fn reader_palette_json(&self, paint: &QString, theme_json: &QString) -> QString;

        /// Pages narrower than this get a mail's fixed widths loosened; 0
        /// when it has none. Once per mail, not per resize.
        #[qinvokable]
        fn reader_fit_below(&self, body: &QString) -> i32;

        /// The reader's full HTML document around a sanitized body
        /// (`mailcore::html::reader::document`). `options_json`: `paint`,
        /// `theme` (see `reader_palette_json`), `allow_remote`, `top_space`,
        /// `scale`, `fit`, `extra_css`.
        #[qinvokable]
        fn reader_document(&self, body: &QString, options_json: &QString) -> QString;

        /// A clicked link split for the examine dialog, and whether it may be
        /// opened at all, as JSON (`mailcore::html::link_info`: `safe`,
        /// `scheme`, `host`, `path`).
        #[qinvokable]
        fn link_info_json(&self, url: &QString) -> QString;

        /// The reader text size's factor on HTML mail
        /// (`mailcore::store::settings::reader_text_scale`).
        #[qinvokable]
        fn reader_text_scale(&self, size: &QString) -> f32;

        /// Whether a delete destroys and whether to ask first, as JSON
        /// `{permanent, ask}` (`mailcore::undo::delete_prompt`).
        /// `permanent_json` is one target folder's `delete_is_permanent` per
        /// entry, `null` for a folder not in the feed.
        #[qinvokable]
        fn delete_prompt_json(
            &self,
            confirm_pref: bool,
            bulk: bool,
            permanent_json: &QString,
        ) -> QString;

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

        /// Reply/forward draft for one message in the current folder as JSON
        /// (`mailcore::compose::AnswerDraft`); `mode` is `reply`,
        /// `reply_all` or `forward`. Returns `"{}"` if unknown.
        #[qinvokable]
        fn answer_draft_json(&self, uid: i32, mode: &QString) -> QString;

        /// New-mail draft (just the signature) as JSON, same shape as
        /// `answer_draft_json`.
        #[qinvokable]
        fn blank_draft_json(&self) -> QString;

        /// FTS search over subject/from/body (`[{uid, folder_id, folder,
        /// subject, from, date, snippet, unread, starred, has_attachments}]`,
        /// FTS rank order). `folder` scopes to one folder path (empty = whole
        /// account). Local SQLite read, no network. Returns `"[]"` when the
        /// query is blank or has no usable terms.
        #[qinvokable]
        fn search_json(&self, query: &QString, folder: &QString) -> QString;

        /// Similar messages across the account as JSON (same shape as search_json).
        #[qinvokable]
        fn find_similar_json(&self, folder_path: &QString, uid: i32) -> QString;

        /// Target message's subject for the "Similar to: ..." chip.
        #[qinvokable]
        fn find_similar_subject(&self, folder_path: &QString, uid: i32) -> QString;

        /// The search syntax for the search field's tooltip
        /// (`mailcore::search::SYNTAX_HELP`).
        #[qinvokable]
        fn search_syntax_help(&self) -> QString;

        /// How the search field runs `query` as JSON (`mailcore::search::plan`:
        /// `mode` off/filter/index, trimmed `query`, `hit_limit`, `debounce_ms`).
        #[qinvokable]
        fn search_plan_json(&self, query: &QString) -> QString;

        /// The short-input filter over one list row
        /// (`mailcore::search::filter_matches`).
        #[qinvokable]
        fn search_filter_matches(
            &self,
            query: &QString,
            subject: &QString,
            from: &QString,
            from_name: &QString,
            snippet: &QString,
        ) -> bool;

        /// Whether one list row's raw date passes an `after` (inclusive) /
        /// `before` (exclusive) `YYYY-MM-DD` pair (`mailcore::search::date_passes`).
        /// Empty bounds are unset.
        #[qinvokable]
        fn date_filter_matches(
            &self,
            date_raw: &QString,
            after: &QString,
            before: &QString,
        ) -> bool;

        /// A named date preset (`today` | `week` | `month` | `older_month`) as
        /// JSON (`mailcore::search::date_preset_range`): `{"after":"","before":""}`.
        #[qinvokable]
        fn date_preset_range_json(&self, preset: &QString) -> QString;

        /// The words for an active date quick-filter
        /// (`mailcore::search::date_filter_label`).
        #[qinvokable]
        fn date_filter_label(&self, after: &QString, before: &QString) -> QString;

        /// What an empty message list says (`mailcore::search::empty_list_text`).
        #[qinvokable]
        fn empty_list_text(
            &self,
            searching: bool,
            server_searching: bool,
            quick_filter: bool,
            unfiltered: i32,
            query: &QString,
        ) -> QString;

        /// Server-side search backfill for thin local results: runs IMAP
        /// `TEXT` search per token across the account's folders — or just one
        /// folder when `folder` is set — and fetches missing hits into the
        /// cache (bounded, metadata only). Network runs on the mailclient-net
        /// thread: returns `""` when queued (completion arrives via
        /// `job_finished` with kind `"Search"`, which refreshes the feeds so
        /// the local search re-query picks the hits up), or a busy message
        /// when not queued.
        #[qinvokable]
        fn search_server(self: Pin<&mut Self>, query: &QString, folder: &QString) -> QString;

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

        /// Fetch the inline (`cid:`) images of a message in the current
        /// folder that sync never kept (mail cached before inline bytes were
        /// stored). The finished job reloads the feeds, and the reader's body
        /// then carries them as `data:` URIs. `""` when queued, else why not.
        #[qinvokable]
        fn download_inline_images(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Export one message as a standard RFC 5322 .eml file at `path`.
        /// Downloads attachments first when they are not cached yet.
        #[qinvokable]
        /// `folder_path` names the message's folder (empty = the open one).
        fn export_message(
            self: Pin<&mut Self>,
            folder_path: &QString,
            uid: i32,
            path: &QString,
        ) -> QString;

        /// Suggested filename for exporting a message as .eml
        /// (`folder_path` as for `export_message`).
        #[qinvokable]
        fn suggested_eml_name(&self, folder_path: &QString, uid: i32) -> QString;

        /// Local storage statistics for the Maintenance settings section, as
        /// JSON (`db_bytes`, `message_count`, cached/temp sizes, …). Local
        /// SQLite read, no network.
        #[qinvokable]
        fn maintenance_json(&self) -> QString;

        /// Write a consistent snapshot of the database to `path` (plain path
        /// or `file://` URL from a save dialog). Local-only copy on the net
        /// thread: returns `""` when queued (completion arrives via
        /// `job_finished` with kind `"Maintenance"`), or a busy message.
        #[qinvokable]
        fn export_database(self: Pin<&mut Self>, path: &QString) -> QString;

        /// Delete every staged viewer copy plus stale draft staging dirs.
        /// Local-only; returns what was removed.
        #[qinvokable]
        fn cleanup_temp(&self) -> QString;

        /// Delete cached messages past the newest 200 per folder. Local-only:
        /// the server is never contacted, guarded rows (drafts, unpushed
        /// changes, pending undos, queued sends) are kept, and trimmed mail
        /// returns with the next sync. A net-thread job: returns `""` when
        /// queued (the result arrives via `job_finished` with kind
        /// `"Maintenance"`), or a busy message.
        #[qinvokable]
        fn trim_cache(self: Pin<&mut Self>) -> QString;

        /// Drop cached attachment bytes, keeping names and sizes. Local-only;
        /// files download again on the next open. A job like `trim_cache`.
        #[qinvokable]
        fn evict_attachments(self: Pin<&mut Self>) -> QString;

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

        /// Whether and when opening an `unread` row marks it read
        /// (`store::settings::mark_read_plan` over the two settings):
        /// `{"plan":"off"|"now"|"after","delay_secs":N}`.
        #[qinvokable]
        fn mark_read_plan_json(&self, unread: bool) -> QString;

        /// Flip the starred flag locally; pushed by the next sync.
        #[qinvokable]
        fn toggle_star(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Move a message to Trash (what the delete action means), undoable:
        /// the message leaves the list now and moves on the server after the
        /// grace period, announced via `undo_available`. Spam, mail already in
        /// Trash, and accounts without Trash are destroyed at once instead (a
        /// job; no undo). Returns the status line text or an error.
        #[qinvokable]
        fn delete_message(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Move a message to the Archive folder (one-click archive), undoable
        /// like `delete_message`. The Archive folder is created server-side
        /// on push when the account has none.
        #[qinvokable]
        fn archive_message(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Move a message to any folder of the same account (by path,
        /// subfolders included), undoable like `delete_message`.
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

        /// Bulk delete, same rules and undo as `delete_message`, one IMAP
        /// command for the whole set.
        #[qinvokable]
        fn delete_many(self: Pin<&mut Self>, uids_json: &QString) -> QString;

        /// Bulk archive, undoable like `archive_message`.
        #[qinvokable]
        fn archive_many(self: Pin<&mut Self>, uids_json: &QString) -> QString;

        /// Bulk move to any same-account folder, undoable like `move_message`.
        #[qinvokable]
        fn move_many(self: Pin<&mut Self>, uids_json: &QString, path: &QString) -> QString;

        /// Bulk permanent destroy (`\Deleted` + expunge, one IMAP session).
        /// Returns e.g. `"Deleted 5 permanently"`.
        #[qinvokable]
        fn purge_many(self: Pin<&mut Self>, uids_json: &QString) -> QString;

        /// Permanently destroy search hits across folders in one job
        /// (`[{"folder": path, "uid": n}, ...]`), without switching folder.
        #[qinvokable]
        fn purge_hits(self: Pin<&mut Self>, hits_json: &QString) -> QString;

        /// Mark search hits read or unread across folders (`hits_json` as in
        /// `purge_hits`), via `mailcore::bulk`.
        #[qinvokable]
        fn mark_read_hits(self: Pin<&mut Self>, hits_json: &QString, read: bool) -> QString;

        /// Star or unstar search hits across folders.
        #[qinvokable]
        fn set_star_hits(self: Pin<&mut Self>, hits_json: &QString, starred: bool) -> QString;

        /// Delete search hits across folders: one Undo for every folder that
        /// goes to Trash, one purge job for those that destroy (already
        /// confirmed as permanent by QML).
        #[qinvokable]
        fn delete_hits(self: Pin<&mut Self>, hits_json: &QString) -> QString;

        /// Archive search hits across folders, one Undo.
        #[qinvokable]
        fn archive_hits(self: Pin<&mut Self>, hits_json: &QString) -> QString;

        /// Move search hits across folders to `path`, one Undo.
        #[qinvokable]
        fn move_hits(self: Pin<&mut Self>, hits_json: &QString, path: &QString) -> QString;

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

        /// An image file as a `data:` URL for the composer to show inline
        /// (the sender turns it into a `cid:` part). Returns the URL, or an
        /// error message (not an image type, too large to go inline) —
        /// anything not starting with `data:` is the error.
        #[qinvokable]
        fn image_data_url(&self, path: &QString) -> QString;

        /// Whether a file would be offered as an inline image (by type).
        #[qinvokable]
        fn is_inline_image(&self, path: &QString) -> bool;

        /// An address split for the From field as JSON `{local, domain}`
        /// (`domain` keeps its `@`), see `mailcore::compose::sender_parts`.
        #[qinvokable]
        fn sender_parts_json(&self, address: &QString) -> QString;

        /// The address a From field sends as: `local` on the account's
        /// domain, or the account address when blank.
        #[qinvokable]
        fn effective_from(&self, local: &QString, account_email: &QString) -> QString;

        /// What the composer's body will be sent as under `format`
        /// (`mailcore::compose::editor::send_format_note`).
        #[qinvokable]
        fn send_format_note(&self, format: &QString, html: &QString) -> QString;

        /// Full Composer form for a draft in the current Drafts folder.
        /// Opening a draft explicitly downloads and materializes its files.
        #[qinvokable]
        fn draft_form(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Destroy a server draft (`\Deleted` + expunge, never filed to
        /// Trash). What the composer's Discard means for a draft opened
        /// from the Drafts folder.
        #[qinvokable]
        fn delete_draft(self: Pin<&mut Self>, uid: i32) -> QString;

        /// Unsent mail for the current account as JSON
        /// (`mailcore::outbox::list_json`: `[{id, status, state, last_error,
        /// retries, retryable, dismissable, has_bytes, envelope_from, envelope_to,
        /// subject, created_at, updated_at}]`). Local SQLite read, no
        /// network. Returns `"[]"` when nothing is queued.
        #[qinvokable]
        fn outbox_json(&self) -> QString;

        /// The current account's outbox counts as JSON
        /// (`mailcore::outbox::status`: `{queued, sending, failed,
        /// retryable, pending, label, has_failures}`). Local read; `"{}"`
        /// without an account.
        #[qinvokable]
        fn outbox_status_json(&self) -> QString;

        /// Forget one queued send (a failed send the user owns the retry
        /// for, or a stale entry). Local-only; returns `""` or an error.
        #[qinvokable]
        fn dismiss_outbox(&self, id: i64) -> QString;

        /// Drop all pooled IMAP sessions (app quit). No LOGOUT round-trip,
        /// so quit never blocks on a dead connection — closing the sockets
        /// reaps the server-side sessions, like any network drop.
        #[qinvokable]
        fn disconnect_all(&self);

        /// Ask one account's IMAP server for its CAPABILITY list (About view).
        /// Network runs on the mailclient-net thread: returns `""` when queued
        /// (the JSON payload arrives via `job_finished` with kind
        /// `"Capabilities"`), or a busy/error message when not queued.
        /// The payload is always JSON:
        /// `{account_id, email, imap_host, imap_port, capabilities[], error}`.
        #[qinvokable]
        fn refresh_server_capabilities(self: Pin<&mut Self>, account_id: i64) -> QString;
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
        #[qproperty(QString, link_click_action)]
        #[qproperty(QString, start_view)]
        #[qproperty(i32, sync_interval_minutes)]
        #[qproperty(bool, quiet_hours_enabled)]
        #[qproperty(QString, quiet_hours_start)]
        #[qproperty(QString, quiet_hours_end)]
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

        /// Persist current properties to the settings store; `""` or the error.
        #[qinvokable]
        fn save(self: Pin<&mut Self>) -> QString;

        /// One account's settings as JSON: `overrides` (what it sets itself)
        /// and `effective` (what applies), `{}` on error.
        #[qinvokable]
        fn account_settings_json(&self, account_id: i64) -> QString;

        /// Write one account's overrides from a JSON object of key to string
        /// value (`""` inherits the app-wide value); `""` or the error.
        #[qinvokable]
        fn set_account_settings(&self, account_id: i64, json: &QString) -> QString;

        /// The automatic check interval that applies to an account (minutes,
        /// 0 = manually).
        #[qinvokable]
        fn sync_interval_for(&self, account_id: i64) -> i32;

        /// Every preference's default and offered values as JSON
        /// (`mailcore::store::settings::choices`); the form only labels them.
        #[qinvokable]
        fn choices_json(&self) -> QString;

        /// A typed quiet-hours time in its stored form (`"7:05"` →
        /// `"07:05"`), or `""` when it does not read as one.
        #[qinvokable]
        fn quiet_time_value(&self, text: &QString) -> QString;
    }

    impl cxx_qt::Threading for Bridge {}
}

use core::pin::Pin;
use std::cell::OnceCell;

use cxx_qt_lib::QString;
use mailcore::feed;
use mailcore::store;

pub(crate) fn qstring(s: &str) -> QString {
    QString::from(s)
}

thread_local! {
    /// The calling thread's connection, opened on first use.
    static DB: OnceCell<&'static mailcore::Db> = const { OnceCell::new() };
}

/// The connection for this thread, opened once and reused.
///
/// Opening a connection costs about 2ms -- the file, the WAL setup and the
/// migration check -- to then do work measured in microseconds, and the
/// bridge is entered on every star toggle, every message selection and every
/// keystroke in the search box. rusqlite's `Connection` is not `Sync`, so a
/// single shared handle is out; the connection is cached per thread instead.
///
/// It is leaked deliberately. Only the GUI thread and the one long-lived net
/// thread ever get here, both live as long as the process, and a thread-local
/// holding a `Db` would otherwise have to be handed out through a closure at
/// every call site. A failed open caches nothing, so the next call retries.
pub(crate) fn shared_db() -> Result<&'static mailcore::Db, String> {
    DB.with(|cell| {
        if let Some(db) = cell.get() {
            return Ok(*db);
        }
        let opened = mailcore::Db::open(&mailcore::default_db_path()).map_err(|e| e.to_string())?;
        let db: &'static mailcore::Db = Box::leak(Box::new(opened));
        let _ = cell.set(db);
        Ok(db)
    })
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
    /// The current folder's "Show older" state (`mailcore::feed::older_state`).
    messages_older: QString,
    messages_can_load_older: bool,
    sort_field: QString,
    sort_descending: bool,
    busy: bool,
    app_version: QString,
    app_license: QString,
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
            messages_older: qstring(""),
            messages_can_load_older: false,
            sort_field: qstring("date"),
            sort_descending: true,
            busy: false,
            app_version: qstring(env!("CARGO_PKG_VERSION")),
            app_license: qstring(env!("CARGO_PKG_LICENSE")),
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
    let (msgs, total, server_total, older) = if folder_id >= 0 {
        let cached = store::messages::count_by_folder(db, folder_id).unwrap_or(0);
        let server = store::folders::get(db, folder_id)
            .ok()
            .and_then(|folder| folder.server_total);
        let msgs = feed::messages_list_json_paged(db, folder_id, cached, 0)
            .unwrap_or_else(|_| "[]".to_string());
        (
            msgs,
            cached.min(i32::MAX as u64) as i32,
            server.map_or(-1, |s| s.min(i32::MAX as u64) as i32),
            Some(feed::older_state(cached, server)),
        )
    } else {
        ("[]".to_string(), 0, -1, None)
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
    bridge
        .as_mut()
        .set_messages_older(qstring(older.map_or("", feed::OlderState::as_str)));
    bridge
        .as_mut()
        .set_messages_can_load_older(older.is_some_and(feed::OlderState::can_load));
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
        let result = shared_db().and_then(|db| {
            mailcore::store::contacts::contacts_json(db, &prefix.to_string())
                .map_err(|e| e.to_string())
        });
        qstring(&result.unwrap_or_else(|e| {
            log::warn!("contacts: cannot load suggestions: {e}");
            "[]".to_string()
        }))
    }

    pub fn recipient_segment(&self, text: &QString) -> QString {
        qstring(mailcore::compose::recipient_segment(&text.to_string()))
    }

    pub fn replace_recipient_segment(&self, text: &QString, replacement: &QString) -> QString {
        qstring(&mailcore::compose::replace_recipient_segment(
            &text.to_string(),
            &replacement.to_string(),
        ))
    }

    pub fn update_contact_alias(&self, address: &QString, alias: &QString) -> QString {
        let address = address.to_string();
        let alias = alias.to_string();
        let alias_opt = if alias.trim().is_empty() {
            None
        } else {
            Some(alias.as_str())
        };
        let result = shared_db().and_then(|db| {
            mailcore::store::contacts::set_alias(db, address.trim(), alias_opt)
                .map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => qstring(""),
            Err(e) => qstring(&e),
        }
    }

    pub fn delete_contact(&self, address: &QString) -> QString {
        let address = address.to_string();
        let result = shared_db().and_then(|db| {
            mailcore::store::contacts::delete(db, address.trim()).map_err(|e| e.to_string())
        });
        match result {
            Ok(()) => qstring(""),
            Err(e) => qstring(&e),
        }
    }

    pub fn cleanup_candidates_json(&self) -> QString {
        let result = shared_db().and_then(|db| {
            mailcore::store::contacts::cleanup_candidates(db, 200)
                .map(|cands| serde_json::to_string(&cands).unwrap_or_else(|_| "[]".to_string()))
                .map_err(|e| e.to_string())
        });
        qstring(&result.unwrap_or_else(|e| {
            log::warn!("contacts: cannot load cleanup candidates: {e}");
            "[]".to_string()
        }))
    }

    pub fn delete_contacts(&self, addresses: &QString) -> QString {
        let addresses = addresses.to_string();
        let parsed: Vec<String> = serde_json::from_str(&addresses).unwrap_or_default();
        let refs: Vec<&str> = parsed.iter().map(String::as_str).collect();
        let result = shared_db().and_then(|db| {
            mailcore::store::contacts::delete_many(db, &refs).map_err(|e| e.to_string())
        });
        match result {
            Ok(_) => qstring(""),
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
    link_click_action: QString,
    start_view: QString,
    sync_interval_minutes: i32,
    quiet_hours_enabled: bool,
    quiet_hours_start: QString,
    quiet_hours_end: QString,
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
            link_click_action: qstring("examine"),
            start_view: qstring("folders"),
            sync_interval_minutes: 0,
            quiet_hours_enabled: false,
            quiet_hours_start: qstring("00:00"),
            quiet_hours_end: qstring("07:00"),
            signature_enabled: false,
            signature_text: qstring(""),
            reply_below_quote: false,
            request_mdn: false,
            ui_scale: 1.0,
        }
    }
}

mod accounts;
mod capabilities;
mod composer;
mod folders;
mod messages;
mod outbox;
mod settings;
mod sync;
mod worker;
