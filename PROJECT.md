# PROJECT.md — Mailclient

## 1. Goal

A **full-featured, modern, responsive mail client** for **Omarchy Linux** first, plus Windows (same Qt client) and Android (native Kotlin client):

- Multiple IMAP/SMTP accounts, full folder trees, background sync (polling + instant navigation sync + Omarchy bar widget).
- Send/receive **text + HTML** mail with a nice composer (rich-text editor, attachments, drafts).
- Fast local SQLite cache + full-text search, offline-first.
- Clean, modern QML UI: account/folder sidebar, message list, HTML reader, composer, search, contacts autocomplete.
- Extensible sync layer so future protocols (POP3, JMAP, EWS/Graph) can plug in.

## 2. Architecture

```text
crates/mailapp/qml/ ─QtQuick UI (desktop)─▶ crates/mailapp (cxx-qt bridge, Qt models) ─┐
                                                                                       │
android/ ─────────Compose UI (Android)───▶ crates/mailffi  JNI (src/android.rs) ───────┼─▶ crates/mailcore (db/store/sync/queue)
                                                                                       │
flutter/lib/ ─────Flutter UI (retired)───▶ crates/mailffi  flutter_rust_bridge ────────┘
                                                  ▲
                                  SQLite (~/.local/share/mailclient/mailclient.sqlite)
```

One core, two active frontends: **Qt/QML** on the desktop and **native
Kotlin/Compose** on Android, kept feature-for-feature in sync (§9).
`mailcore` is the base for all of them: behaviour lives there so every
frontend inherits it, and each adapter crate only translates. **Flutter is
retired** — it keeps building, gets obvious bug fixes, no new features
(AGENTS.md §1). On a desktop with Qt and Flutter installed they open the
same database file, on purpose.

- `crates/mailcore`: pure Rust. Modules: `error`, `models`, `compose` (composer send/drafts for both frontends), `undo` (undoable delete/archive/move), `db/{mod,schema,migrations}`, `store/{accounts,folders,messages,pending_moves,queue,contacts}`, `sync/{traits,imap,sender}`, `search`.
- `crates/mailapp`: `cxx-qt` QObject bridge (`Bridge`, `SettingsBridge`, `AccountListModel`, `FolderTreeModel`, `MessageListModel`, composer controller) + `main.rs` loading `Main.qml` (embedded `Mailclient` module, filesystem override via `MAILCLIENT_QML_DIR`).
- `crates/mailapp/qml/`: `Main.qml`, `Sidebar.qml`, `MessageList.qml`, `MessageView.qml`, `Composer.qml`, `AccountSetup.qml`, `Settings.qml`, `components/*`.
- `crates/mailffi`: `cdylib` over `mailcore` for the Android frontends —
  `android.rs` (JNI externs behind `MailNative.kt`) for native, and
  `api/{init,events,accounts,folders,messages,mutate,sync,search,composer,attachments,contacts,settings}`
  (flutter_rust_bridge) for Flutter, plus `net` (the shared `mailclient-net`
  thread), `session` (IMAP pool) and `db` (per-thread handle). No mail
  logic, no Qt.
- `android/`: the native Android app (Kotlin + Jetpack Compose) — shell,
  folders, list, reader, composer, accounts, contacts, settings, outbox,
  plus native background checks, push and notifications. See
  `android/README.md`.
- `flutter/`: the retired Dart app — `src/ffi` (library loading + generated
  bindings), `src/models`, `src/state`, `src/ui/*`. See `flutter/README.md`.
- `scripts/`: `install-local.sh`, `qt-env.sh`, `smoke.sh`. Output bundle: `dist/mailclient/`.
- `flutter_rust_bridge.yaml` (repo root): FFI codegen config, with the
  Windows twin `flutter_rust_bridge.windows.yaml` (backslash paths). At the
  root rather than in `flutter/` because the tool does not normalise a
  leading `..`.

See `AGENTS.md` for agent rules, dependency policy, and Definition of Done.

## 3. SQLite Schema

`crates/mailcore/src/db/schema.sql` is authoritative — it is the DDL a fresh
install runs, so it is always the current shape. The table below is a reading
aid, not a specification; where the two disagree, the file is right. The
version an existing database is upgraded to lives in `SCHEMA_VERSION`
(`db/migrations.rs`), and columns are only ever added through a new
`migrate_vN` step, never by editing a released one.

| Table | Purpose | Key columns |
|---|---|---|
| `schema_meta` | migration version | `key`, `value` |
| `accounts` | per-account IMAP/SMTP config, **no passwords** | `id`, `name`, `email_address`, `from_name` (sender display name, `''` = address only), `imap_host/port/security/username`, `smtp_host/port/security/username`, `auth_vault_key`, `check_interval_secs`, `created_at`, `updated_at` |
| `folders` | IMAP folder tree per account | `id`, `account_id→accounts`, `path`, `delimiter`, `role` (inbox/sent/drafts/trash/junk/archive/custom), `uid_validity`, `uid_next`, `server_total` (last SELECT's count, drives "load older"), `highest_modseq` (CONDSTORE/QRESYNC delta point), `subscribed`, `last_sync_at` |
| `messages` | cached headers + bodies | `id`, `account_id`, `folder_id→folders`, `uid`, `message_id_header`, `thread_id`, `subject`, `from_addr`, `to_addrs/cc/bcc/reply_to` (JSON), `date`, `snippet`, `body_text`, `body_html`, `raw_headers` (technical-headers view), `flags` (`is_read/is_starred/is_draft/has_attachments`, `keywords` JSON), `flags_dirty` (local read/star change awaiting its IMAP push — a toggle must never wait on the network), `size`, `downloaded_full`, UNIQUE `(account_id, folder_id, uid)` |
| `messages_fts` | FTS5 full-text index, external content over `messages` | `subject`, `from_addr`, `body_text`, `rowid` = `messages.id`. Kept in step by insert/delete/update triggers; the update trigger fires only when indexed text actually changed, so a flag write does not re-index the body |
| `attachments` | names/sizes synced, bytes only on explicit request (SQLite BLOB cache) | `id`, `message_id→messages`, `filename`, `mime_type`, `size`, `content_id`, `data` (BLOB, `NULL` until downloaded), `is_inline`, `storage_path` (legacy disk pointer, unused by new code) |
| `contacts` | autocomplete (built from mail) | `address` PK, `name` (as transferred), `alias` (user-editable override), `times_seen`, `last_seen_at` |
| `pending_moves` | undoable delete/archive/move waiting out the grace period; hides the message from every list, count and search until pushed | `message_id→messages` PK, `batch` (the Undo handle), `action` (trash/archive/move), `dest_folder_id→folders` (NULL = resolve Trash/Archive at push), `due_at`, `attempts`, `created_at`, `updated_at` |
| `send_queue` | outbox for reliable sending | `id`, `account_id`, `message_id→messages`, `status` (queued/sending/sent/failed), `last_error`, `retries`, `raw_mime` + `envelope_from` + `envelope_to` (the built message, so a send survives a crash), `created_at`, `updated_at`. `sending` means "owned by a submitter": rows are born `sending` (claimed by their creator), and a flush only takes `queued`/`failed` rows through the atomic `queue::claim` |
| `settings` | user preferences | `key` PK, `value`. Keys and defaults are defined in `store::settings`, which is where to look rather than here |
| `account_settings` | per-account overrides of sync preferences; a missing row inherits the app-wide value | `account_id→accounts`, `key` (one of `store::account_settings::KEYS`), `value`, `created_at`, `updated_at`, PK `(account_id, key)` |

Secrets live in the OS keyring keyed by `accounts.auth_vault_key`, never in SQLite.

## 4. Where We Stand (update as we go)

| # | Milestone | Status |
|---|---|---|
| 0 | Repo scaffold: workspace, `mailcore` schema + CRUD, `mailapp` cxx-qt skeleton, QML shell, `./dev.sh`/`./build.sh`/`scripts/install-local.sh`, `dist/` bundle | ✅ done |
| 1 | Real IMAP sync + app wiring: account setup (keyring), LIST/SELECT/FETCH, UIDVALIDITY handling, flag push/delete, send + Sent-copy, live folder/message feeds in QML | ✅ done (verified live against test account) |
| 2 | Composer polish: drafts, attachments, full rich-text editor (toolbar wraps selection today) | ✅ done (rich HTML compose + source view + Cc/Bcc + auto send-format + attachments; server drafts save/replace/open with close-time Save offer and Drafts auto-create) |
| 3 | Reader/search: FTS search UI, remote-image handling polish | ✅ done (safe sanitized HTML reader + remote-block banner + show-once; toolbar search runs the FTS5 index account-wide from 3+ letters with jump-to-message results, short input keeps the instant folder filter; thin local hits trigger a debounced IMAP `TEXT` backfill into the cache, later syncs keep backfilled mail) |
| 4 | Contacts + settings UI extras (threading, notifications explicitly dropped — not needed) | ✅ done (contacts manager, About with version/licence + server capabilities) |
| 5 | Polish: background polling sync, offline/error states, onboarding, `.desktop`/icons, Windows feasibility | ✅ done (polling + pooled sessions + manual ⟳ done; offline-first cache + status-bar errors done; empty-state setup onboarding done; `.desktop`+icon installed; IDLE dropped; Windows portable via MSYS2/Git Bash scripts, built as a GUI-subsystem exe with its own icon linked in, so launching it opens no console window; headless `--sync-once`/`--status` JSON + Omarchy bar widget `mailclient.unread` done, see below) |
| 6 | Bar integration: shared `mailcore::sync::headless` (GUI + CLI same orchestration), `mailapp --sync-once/--status [--json]`, cross-process `.sync.lock` (serializes `--sync-once` runs; the GUI never takes it, the outbox claim keeps GUI + CLI overlap safe), `resources/omarchy/mailclient/` bar-widget plugin (status poll + sync timers, notify-on-rise, click-to-open) | ✅ done |
| 7 | Flutter frontend (**retired** — superseded on Android by milestone 14; builds kept, bug fixes only): `mailffi` cdylib (flutter_rust_bridge 2, in-process `dart:ffi`), Dart app in `flutter/` with responsive 3/2/1-pane shell, sidebar, list, reader, account setup; CMake wiring for Windows + Linux, Gradle wiring for Android | ✅ done (full parity with QML: link safety & hover URL overlay, row action menus with passive stars, composer & outbox resilience, Linux release bundle + signed Android APK) |
| 8 | Maintenance settings section in both frontends: storage stats (DB/messages/cached/temp sizes), database export (`VACUUM INTO` snapshot), temp cleanup, downloaded-file eviction (re-downloads on open), local-only cache trim to the newest 200 per folder (drafts/unpushed/pending/queued rows kept, server never contacted) — all in `mailcore::maintenance`, frontends are UI only | ✅ done |
| 9 | Outbox visibility: shared `mailcore::outbox` (counts, list metadata, dismiss, one-line state), status-bar pill (count + red on failure) and outbox dialog (rows, error, retry-via-sync, dismiss dead rows) in both Qt/QML (`Outbox.qml`) and Flutter (`OutboxDialog`) | ✅ done |
| 10 | Find similar messages: shared `mailcore::similar` (3-tier matching: same `thread_id`, same sender + normalized subject, subject keywords via FTS5 ranked best-first; self-exclusion, account-wide scope, JSON matching `search_json`), exposed via Qt bridge and `mailffi`/FRB, with "Find similar" in row and reader menus and dismissable `Similar to: "<Subject>" ✕` chip in both Qt/QML (`MessageList.qml`) and Flutter (`MessageListPane`) | ✅ done |
| 11 | Export message as .eml: shared `mailcore::export` (`assemble_eml`, `export_eml_to`, `suggested_eml_name` — RFC 5322 MIME reconstruction preserving routing/technical headers, body tree shared with the composer's lettre `mime_body`, missing attachments downloaded first via `export::prepare`, safe filename sanitization), exposed via Qt bridge and `mailffi`/FRB, with "Save as .eml…" in row context menus and reader action menus across Qt/QML and Flutter | ✅ done |
| 12 | Calendar / ICS event preview card: pure-Rust RFC 5545 `VEVENT` parsing in `mailcore::calendar` (unfolding, quote-aware parameter & unescaping parsing, nested `VALARM` ignored, TZID labels / fixed-offset `VTIMEZONE`, exclusive all-day ends, formatted date ranges, cancellation detection), instant offline preview via small .ics sync caching in `store_attachment_meta`, reader event preview card with summary, date/time, location, organizer, cancellation badge and "Open in Calendar" / "Save .ics…" actions across Qt/QML (`EventCard.qml`) and Flutter (`EventCard`) | ✅ done |
| 13 | Search refinements: structured query tokens (`is:unread`, `is:read`, `is:starred`, `is:flagged`, `has:attachment`, `after:YYYY-MM-DD`, `before:YYYY-MM-DD` and negations) in `mailcore::search` (`parse_query_full`, `SearchFilters`, `ParsedQuery`), filter-only non-FTS query execution & combined SQL filtering in `feed::search_json`, IMAP criteria translation in `imap_criteria` (only with a positive text term; flags/dates narrow it, `has:` stays local-only); quick filters stay in the list header's filter menu in both frontends | ✅ done |
| 14 | Native Android frontend (Kotlin + Jetpack Compose, `android/`): the Android client, kept in step with Qt — same `mailcore` over JNI (`MailNative` ↔ `mailffi/src/android.rs`, package `de.renier.mailclient` is JNI-bound), native background checks, push and notifications, Compose BOM pinned in `android/app/build.gradle.kts`, `./build.sh --android` signed APK into `dist/mailclient-android/` | ✅ done — every screen of the Qt client exists (shell, folders, list, search, reader, composer, accounts, contacts, settings, outbox); remaining Qt ↔ native differences are tracked in §9 "Open Qt ↔ native gaps" |

Milestone 7 detail — what works and what does not:
- **Works**: the whole read path and the local write path. Accounts
  (list/add/edit/delete, keyring), folders (tree, unread pills, subscription),
  messages (paged list, reader with sanitized HTML and the blocked-remote-images
  "show once", attachment bar), local flag writes with background push, queued
  sync / folder sync / load-older / folder refresh, queued delete / archive /
  move / purge, FTS search plus server backfill, contacts, settings. The Rust
  API for send, drafts and attachment download is implemented and tested for
  what it can be offline.
- **Built in the UI**: composer (plain-text; reply/reply-all/forward prepared
  by `mailcore::compose::answer`, the quoted original carried beside the text
  box as HTML, collapsible and removable, drafts edit/save/delete, contact autocomplete, discard confirm,
  editable From local part with the account domain locked, sender name
  prefilled from the account, real file picker for outgoing attachments,
  re-entry and double-send protection while SMTP submission or draft saving
  is pending), settings screen (all keys incl. sort, signature, intervals,
  `link_click_action`) with About (version/licence/database + per-account IMAP
  capabilities), search UI (FTS with folder/account scope + debounced server
  backfill + jump-to-message), contacts manager (search, alias, remove),
  multi-select with bulk bar (read/unread/star/archive/move/trash/purge + select
  menus + shift-range), folder manager (create, show/hide, refresh, open),
  move picker, accounts manager, reader actions (reply/forward/star/archive/move/delete,
  headers dialog, fullscreen on wide layouts, sender display name from the stored
  headers) with working attachment Open/Save/Save-all, sort menu,
  delete/purge confirms, auto-sync timer, mark-read delay, interface scale,
  resizable sidebar/list panes, avatar + unread-dot rows, message list `⋮` row
  actions menu with passive star cue beside sender, reader floating link hover
  statusline overlay, link safety policy and Examine link dialog with external
  `url_launcher` navigation.
  On Linux the file dialogs need zenity, kdialog or qarma installed —
  without one the picker says so instead of failing silently.
- **Deliberately simpler than QML**: plain-text composer instead of WYSIWYG
  (auto send format stays plain), no global Up/Down/Delete/R/F keyboard
  map beyond compose/sync/search-focus shortcuts.
- **Not verified against a live server**: nothing in the Flutter path has been
  run against a real mailbox yet. `cargo test`, `cargo clippy -D warnings`,
  `flutter analyze` and `flutter test` are clean, and `./build.sh --flutter`
  / `--apk` produce runnable release bundles (Linux with `libmailffi.so`,
  signed Android APK), but a live run needs explicit
  per-run consent (`AGENT.md` §2).
- **Android APK builds**: `./build.sh --apk` cross-compiles `mailffi` for
  arm64-v8a / armeabi-v7a / x86_64 via cargo-ndk and packages a signed release
  APK into `dist/mailclient-apk/`. Signing reads the gitignored
  `flutter/android/key.properties`; the NDK resolves from `ANDROID_NDK_HOME`
  or `<sdk.dir>/ndk/<flutter.ndkVersion>`. TLS is rustls-only (no OpenSSL) so
  the cross-compile works. Secrets on Android live in an app-private
  `auth_vault.json`, not the Android Keystore — a stated simplification, see
  `flutter/README.md` ("Android"). Not yet run on a device against a live
  mailbox.
- **Shared, not duplicated**: the composer (validation, send settings,
  exactly-once outbox, Sent copy, draft save/replace/delete) lives once in
  `mailcore::compose`; both adapters only start jobs and phrase results.
  Account saving is `mailcore::store::account_form`, the IMAP session pool
  `mailcore::sync::pool`. The reader's HTML document and dark rewrite
  (`mailcore::html::reader`), link safety, settings choices and defaults,
  the search plan, folder rules and cross-folder bulk actions are core
  functions too; `SHARED-CORE.md` lists what moved and what is still open.
  The small items followed: attachment size text, `date_key`-driven
  "Yesterday", folder depth/short name, the mark-read-on-open plan, the send
  job outcome, and quote-aware recipient segments — all core decisions now,
  with nothing open left in that file.

Current state detail:
- `mailcore`: the SQLite schema of §3 (FTS5 index, `settings` key/value store, local-change queueing via `messages.flags_dirty`, attachment bytes cached as BLOBs), typed stores (accounts/folders/messages/queue/contacts/settings incl. `compose_send_format`), `html` sanitizer (std-only tokenize→clean→serialise, remote/private-host gating, entity-aware incl. `&nbsp;`), IMAP sync (SPECIAL-USE role mapping, windowed UID FETCH + MIME parsing incl. Reply-To capture — INBOX newest 200, others newest 50 auto / 200 on open — UIDVALIDITY resync, expunge, flag refresh/push, server-side delete, Sent-copy APPEND, attachment names/sizes extracted with 25 MiB/file + 50-file caps — bytes never auto-download, only `fetch_attachments` on explicit Open/Save spends bandwidth). The Date list order follows the shown `Date:` header (undated last, UID as tiebreaker), not the UID alone — a moved mail gets a fresh UID in its new folder. SMTP send with `SendPolicy` + `SendFormat` (auto/plain/multipart/html, resilient fallback to auto; Auto sends text/plain unless the body carries real formatting, with an optional plain twin via `compose_include_plain`; Cc + Bcc; blank/placeholder To sends `To: undisclosed-recipients:;` (or `To: <text>:;`) with the envelope from Cc/Bcc; sender display name from the composer or account default; EHLO uses the sender domain; sanitized outgoing, `multipart/mixed` file attachments with extension-guessed MIME), keyring auth on desktop (app-private vault file on Android), safe JSON feeds (`body_text`/`body_html` sanitized/`is_html`/`has_remote_images` + legacy `body`, plus on-demand `message_html` for Show-once; message rows carry `has_attachments` + attachment metadata, never bytes).
- `mailapp`: `Bridge` (accounts, persisted last-active account restored at startup, active-account-only sync on switch, selective sync + per-folder `sync_folder_now` + `load_older_messages` paging, folder LIST refresh + `subscribed` visibility, select/read/star/delete/send/archive/move with Cc/Bcc + format-aware bodies + composer file attachments, on-demand `message_html`, attachment open/save/save-all to disk, compact paged list feeds plus an on-demand full reader payload so folder navigation never sanitizes 200 bodies, persisted list sort + bulk mark/star/delete/archive/move/purge) + `SettingsBridge` (`sent_copy_enabled`, `load_remote_images`, `compose_send_format`, `compose_include_plain`, `auto_mark_read`, `mark_read_delay_secs`, `collect_sent_contacts`, `confirm_delete`, `list_density`, `reader_font_size`, `link_click_action`, `sync_interval_minutes`, `signature_enabled`/`signature_text`, `reply_below_quote`, `request_mdn`, `ui_scale`; per-account overrides via `account_settings_json`/`set_account_settings`/`sync_interval_for`), `Bridge` app version/license (`app_version`/`app_license` from the package manifest) + per-account live IMAP CAPABILITY viewer (`refresh_server_capabilities` on the net thread, JSON via `job_finished`), embedded `Mailclient` QML module with filesystem override.
- `qml`: live 3-pane UI — real folders/messages, working sync/send/reply/forward/star/delete/archive, account setup with ports+encryption, Roundcube-style settings (Interface / Mailbox / Reading / Composing / Accounts & sync / About sections, scrollable, Cancel truly reverts) with interface scale, delete-confirm, list density, reader text size, auto-check interval, signature, reply above/below quote and read-receipt request, About showing the app version + short licence info alongside the per-account IMAP server capabilities (account picker + Refresh, pill list), IMAP folder manager (LIST refresh, show/hide per folder, create folders, cached/unread counts). Reader: PlainText for plain (no more HTML-code display), sanitized WebEngine + blocked-images banner + working show-once, link hover URL + examine-first safety dialog (or direct browser open, per `link_click_action`) with the reader never navigating away, (inline `cid:`/`data:` always load, remote gated + re-sanitized on demand), attachment bar with Open (downloads-if-needed to a temp copy for the system viewer) + per-file Save + Save-all (bytes download on first request, then cache as SQLite BLOBs → disk via save dialogs). `⋯` menu holds Reply-all/Archive/Move plus a Headers dialog (From/To/Cc/Date/Subject/Message-ID/Reply-To). A differing Reply-To is shown inline in the reader and as a banner when replying, so answering visibly goes to the right address. List shows 📎 for mails with files and pages in sort order with a "Show older messages" button (one 200-mail server batch per press), plus Roundcube-style multi-select (header ☑ toggle reveals checkboxes, Ctrl/Shift range, select all/none/unread/starred/invert, bulk read/unread/star/archive/move/Trash with purge confirm) and sorting (Date/From/Subject, asc/desc, persisted). Startup auto-sync, folder-open fill, post-send refresh of Sent + viewed folder (composer validates + queues locally and closes at once; SMTP submit, Sent copy, draft removal and resync run in the background, failures report as `sent, but …` or reopen the composer with the text kept). Composer: compact headers (From with per-mail sender name + account default / To / Subject + collapsible Cc/Bcc toggles beside To, optional Reply-To behind a collapsed ↩ toggle beside From), WYSIWYG + HTML-source toggle, list/quote/link/clear, Cc/Bcc wired, send-format Auto (plain unless formatted, optional plain twin), file attachments (picker chips, sent as `multipart/mixed`), reply/forward quote in the received format (`>` citations for plain, blockquote for HTML), with recipients (reply-all copies everyone but the account and the target), subject (no stacked `Re:`/`AW:`/`Fwd:`), attribution with the full date, quote and signature prepared once by `mailcore::compose::answer` for both frontends. No mocks remain.
- Verified live: 5 folders mapped, messages synced, test mail delivered + filed to Sent.
- QML is a responsive shell (wide: sidebar / list / reader; medium: sidebar + list-or-reader with a back chevron; narrow: one pane with folder/list/reader navigation, where Back clears the selection and an emptied reader falls back to the list; draggable wide split handles; manual sidebar toggle on wide only). Below a scale-aware width the toolbar goes compact (icon Compose, secondary actions and the folder-search toggle in a ⋮ menu), and Settings collapses its section list to an icon rail. Icons are real vector glyphs from the bundled Material Icons font (`qml/fonts/`, `qml/Icons.qml` single source, loaded once by `Main.qml`, embedded via `qrc_resources` so embedded/dist/dev runs all resolve it); menus use self-rendering `AppMenuItem`s because a Menu only instantiates its delegate for model-driven items. Every model it shows comes from the Rust bridge, so it needs the app to run — there is no mock-data path that renders it standalone. Headless menu/font checks run via `QT_QPA_PLATFORM=offscreen qmltestrunner` with `QML2_IMPORT_PATH` pointed at a stub `Mailclient` module made of copies of the real QML files (throwaway harness, not checked in); checked-in `tst_AccountOverrides.qml` runs under `scripts/qml-check.sh`, which also gates `qmllint` on unknown Qt-framework members (that class fails silently at runtime).

## 5. Sync Strategy (when / what / scaling)

Manual ⟳ plus auto-refresh on startup and after send. Folder switches are
cache-only so they render immediately.

- **When**: cache shows instantly (offline-first); folder clicks only change
  the local feed. Auto-sync runs on startup (deferred past first paint), plus
  a best-effort Sent refresh after each send. The toolbar Sync refreshes the
  account explicitly. Read/star stay local + queued
  (`flags_dirty`) and push on the next sync; a quiet background job also
  pushes them seconds after every toggle (no busy latch, failures stay dirty),
  so quitting right after reading loses nothing. Delete (to Trash), archive and
  move are undoable: `mailcore::undo` queues them in `pending_moves`, which
  hides the mail at once, and the IMAP move runs after `UNDO_GRACE_SECS` from
  the quiet background push or any sync (only rows past their grace period);
  a quit before then leaves them for the next sync. Undo deletes the rows.
  A delete that destroys (Junk, Trash, no Trash folder) and an explicit purge
  still hit IMAP at once, behind a confirm.
- **What**: multi-pass folder discovery every run (recursive `LIST`, `LSUB`
  merge, per-root subtree `LIST` incl. dotted prefixes, `LIST` inside every
  `NAMESPACE` prefix — a single `LIST "*"` missed folders like Archive on
  groupware servers; `\Noselect`/`\NonExistent` skipped, first pass wins role
  mapping, every find logged with raw attributes. Servers whose responses
  the IMAP parser cannot model (proven: Tobit's `* NAMESPACE`, which
  imap-proto 0.10 has no type for) leave a stale tagged reply behind, so the
  session reconnects itself on exactly that parse error before continuing),
  then selective + windowed per folder: INBOX syncs flags + newest 200 full
  bodies (`FULL_SYNC_WINDOW`, matches feed limit); every other folder only
   flags + newest 50 (`QUICK_SYNC_WINDOW`) for fresh sidebar pills — custom
   folders never auto-sync all mail. Delete means Trash, except spam (destroyed outright,
  junk never touches Trash) and Trash itself (deleting there is permanent);
   one-click archive moves to Archive (auto-created server-side when missing).
   Expunge diffing covers the full queried range (local, no extra network):
   the UID SEARCH pages backwards to UID 1 when the range is sparse, so
   server-side deletions anywhere are picked up, not just inside the newest-N
   window;
   UIDVALIDITY resync on change. Attachment bytes are never fetched during
   sync — only names/sizes land in SQLite, so ⟳ stays cheap no matter how
   large the files are. A file downloads exactly once, on explicit user
   request (Download / Save / Save-all in the reader), then caches as a BLOB
   for offline use; resyncs never wipe downloaded bytes.
- **Scaling (massive mailboxes)**: per-folder network is bounded by the window
  (a bounded handful of UID SEARCH pages + ≤200 flag FETCH + ≤200 RFC822 FETCH), not by mailbox size.
  Fast CONDSTORE/QRESYNC deltas with automatic RFC 3501 fallbacks, local expunge diffing, and polling. (IDLE dropped).

## 5a. COMPLETED — IMAP Resilience, imap-next Migration & Fallback Architecture

**Status: Complete.**

### Architecture & Implementation
- Replaced the legacy blocking `imap` / `imap-proto` stack with `imap-next` and `imap-types`, adopting a sans-I/O state machine driven from Tokio. Pinned versions are in `crates/mailcore/Cargo.toml`.
- Migrated the TLS layer to `tokio-rustls`, trusting the platform store via `rustls-native-certs` and falling back to `webpki-roots`.
- Plaintext connections are refused unless the account configuration explicitly specifies `imap_security = "plain"` or `"none"`.
- Added `folders.highest_modseq` through a new migration step, so each folder remembers the modseq its last sync saw.
- Implemented CONDSTORE and QRESYNC support with multi-source capability detection:
  - Multi-source capability parsing (`Data::Capability`, untagged `* OK [CAPABILITY ...]`, and tagged `OK [CAPABILITY ...]`).
  - RFC 5161 `ENABLE` guard: client only issues `ENABLE` if `has_capability("ENABLE")` is true.

### Defensive Runtime Fallbacks
- **`SELECT` Fallback**: Tries `SELECT (QRESYNC/CONDSTORE)` when enabled. If rejected with `BAD`, catches the failure, disables `qresync_enabled`/`condstore_enabled`, and retries with standard RFC 3501 `SELECT`.
- **`CHANGEDSINCE` Fallback**: If `UID FETCH ... (CHANGEDSINCE)` is rejected with `BAD`, catches error, disables `condstore_enabled`, and retries with standard `UID FETCH (UID FLAGS)`.
- **`MOVE` Fallback**: Checks `has_capability("MOVE")`. If absent, or if `UID MOVE` fails at runtime, falls back to `UID COPY` + `UID STORE \Deleted` + an expunge that is scoped to the moved UIDs wherever UIDPLUS allows it (see the UIDPLUS note below).
- **Local Expunge Diffing**: Always diffs queried server UIDs against local SQLite rows (`*uid >= search_lo && !server_uids.contains(uid)`), catching server-side expunges at zero extra network cost.

### Trash "Always Seen" Semantics
- **Immediate Server Sync**: When moving messages to Trash (via Delete, Bulk Delete, or "Move to..."), `UID STORE +FLAGS (\Seen)` is executed immediately on the server before moving.
- **Trash Sync Enforcement**: During `sync_folder_window` on Trash, all messages are forced to `is_read = true`, and any unread UIDs found on the server are marked `\Seen` immediately.
- **Feed Representation**: Sidebar feeds report `unread: 0` for Trash, and message feeds report `unread: false`.

### Non-Blocking Composer & Delivery
- Converted `save_sent_copy`, `flush_outbox`, and `send_raw` to fully `async` functions, removing `tokio::task::block_in_place` (which caused panics and UI lockups on Tokio's `current_thread` runtime).
- Exactly-once delivery across processes: `enqueue_mime` inserts rows already claimed (`sending`), `flush_outbox` must win `queue::claim` (one conditional `update`) before any SMTP, and `requeue_interrupted` only recovers `sending` rows idle long enough to be crash orphans — never a live submit from the other process.
- The GUI refuses a send while busy *before* the row exists, and drops the row's MIME on every failure before submit, so a message the user saw fail is never delivered later behind their back.
- The single composer is guarded per composition: while a send awaits SMTP acceptance or a draft saves, new compositions are refused and editing is locked, and results only act on the composer while their own request is still pending.

### Dropped Features (And Why)
- **IMAP IDLE (RFC 2177)**: Dropped on the desktop (not needed): periodic background polling + instant navigation sync on folder click + Omarchy bar widget (`mailclient.unread`) satisfy real-time mail needs there. Adopted on Android as the opt-in Push scheduler (`mailcore::sync::push`, F26), where polling a sleeping phone costs more battery than holding an idle connection.
- **Background System Daemon**: Dropped (not needed). The Omarchy bar widget already runs headless periodic syncs in the background, and the desktop app retries pending sends while open. An external system service/daemon is redundant.
- **UIDPLUS (RFC 4315)**: Partly adopted, for `UID EXPUNGE` only. It was dropped for discovery, and that still holds — standard folder refresh plus local UID diffing find new and moved messages without it. Deletion is different: flagging `\Deleted` and then issuing a mailbox-wide `EXPUNGE` also destroys whatever another client has flagged but not yet expunged, i.e. someone else's pending, still-undoable delete. Where the server advertises UIDPLUS we expunge by UID; where it does not, we fall back to the mailbox-wide form, which is why that hazard is a fallback and not the default.
- **Streaming Partial MIME Parts (RFC 3516 / BINARY)**: Dropped (not needed). Single-roundtrip full RFC 822 body fetch is faster and simpler for normal desktop workloads; attachments are capped at 25MB and downloaded on demand.
- **Optimistic Concurrency (`UNCHANGEDSINCE`)**: Dropped (not needed). Desktop user actions are authoritative; newest state reconciles on next fetch without conflict retry loops.

### Verification
- The whole suite runs offline in about a second and makes zero live network calls: `mailcore`'s tests drive an in-memory raw IMAP wire-protocol mock server and an in-memory SQLite, and `mailapp`'s cover the bridge's own logic. `cargo test --workspace` is the check; a test that dials out does not belong here (see AGENT.md).
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` are clean; both are part of finishing a change (see AGENT.md).
- Release bundle builds cleanly via `./build.sh --qt` into `dist/mailclient/bin/mailapp`.

## 6. Build / Run / Install

```sh
./dev.sh                # Qt dev: debug build + run (uses ./crates/mailapp/qml live)
./dev.sh --flutter      # Flutter dev: flutter run -d linux (debug Dart, release Rust core)
./build.sh --qt         # Qt release build → dist/mailclient/{bin/mailapp,qml/,resources/}
./build.sh --flutter    # Flutter release build → dist/mailclient-flutter/{mailclient,lib/,data/}
./build.sh --apk        # signed Android APK → dist/mailclient-apk/mailclient-<version>-release.apk
./build.sh --all        # both desktop release bundles (Qt + Flutter Linux)
./scripts/install-local.sh  # copy Qt bundle to ~/.local/{bin,share/mailclient} + install .desktop
cargo test -p mailcore      # backend unit tests (SQLite in-memory)
qmllint crates/mailapp/qml/*.qml crates/mailapp/qml/components/*.qml  # QML lint (uses /usr/lib/qt6/bin when on PATH)
```

Dev runs (both frontends) open the local `./data/dev.sqlite` (override
`MAILCLIENT_DB` for a throwaway file), never the real mailbox. Release
bundles use the platform database (`~/.local/share/...`) unless
`MAILCLIENT_DB` is set when launching them.

DB location: `~/.local/share/mailclient/mailclient.sqlite` (override `MAILCLIENT_DB=/tmp/x.sqlite` for tests/dev).

UI iteration: `./dev.sh` runs the app against live `crates/mailapp/qml/` (embedded module is the fallback). Every pane now does `import Mailclient` for the `Theme` singleton and the Rust QObjects, so no component previews standalone under `qml6` — iterate through `dev.sh`, which rebuilds and re-embeds on each run. Static checking is `qmllint` (its "Member not found on type Theme" noise is only the module not being importable outside the binary; the `Quick.layout-positioning` warnings are real).

## 7. Roadmap Notes

- Sync engine behind `SyncProvider` trait; IMAP first, JMAP/POP3 later without touching UI.
- HTML compose editing: `TextArea` rich-text now, consider WebEngine-based editor in M2.
- Windows: keep all paths via `directories`, no Linux-only calls outside `mailapp` platform shim.
- Completed: CONDSTORE/QRESYNC delta sync, the `imap-next` tokio migration, capability guards, fallbacks, and trash seen sync — see §5a, which is the full record. A separate background daemon is explicitly dropped, IDLE on the desktop too (Android push uses it, F26); UIDPLUS is used for `UID EXPUNGE` and nothing else (§5a).

## 8. Known Flaws & Repair List (user-reported)

F1–F17 below; all currently closed.

| # | Flaw | Status | Resolution |
|---|---|---|---|
| F1 | Cannot select other mails in the list (selection stuck / jumps back) | ✅ fixed | Selection is a UID, not a row index: clicks report `messageSelected(uid)`, the highlight derives from `currentUid`, and `onCurrentIndexChanged` no longer re-emits selection. Feed rebuilds can no longer move it. `open_message` also stopped opening an IMAP connection per click (see F8) |
| F2 | No body shown in reader (empty pane) | ✅ fixed | Plain bodies render in a `Flickable` + sized `TextEdit`; the old `ScrollView` + unsized `TextEdit` had no height and drew nothing |
| F3 | Composer is not WYSIWYG (Bold etc. don't reflect visually) | ✅ fixed | `components/EditorFrame.qml`: a `contentEditable` WebEngine document driven by `execCommand`, so text changes visibly and the toolbar buttons light up from `queryCommandState`. QML's rich-text `TextArea` has no selection-formatting API, which is why the old toolbar could only insert literal tags |
| F4 | Composer fields are placeholder-only, no labels | ✅ fixed | Shared `components/FormField.qml` labels From/To/Cc/Subject (and every dialog field) |
| F5 | Cancel loses the composed entry without asking | ✅ fixed | `dirty` tracking on all fields + a "Discard draft?" confirm; prefilled reply/forward quotes do not count as unsaved work |
| F6 | Overall UI looks sterile, flawed vs modern clients | ✅ fixed | `qml/Theme.qml` design-token singleton (light/dark, spacing, radius, type) + full redesign: list delegates with unread dot, hover, accent bar and inline star; reader header block; sidebar account chip and unread pills; themed dialogs. `main.rs` pins the Basic Controls style so Windows and Linux render identically instead of falling back to the native Windows style |
| F7 | Cannot manage accounts — a mistyped account can only be added, never removed or corrected | ✅ fixed | New `qml/Accounts.qml` manager (list, switch, edit, remove with confirm) on `Bridge.accounts_json` / `select_account` / `delete_account` / `account_form`. Editing keeps the stored password when the field is left blank; removal also drops the keyring secret |
| F8 | Clicking a message froze the UI and could revert read state | ✅ fixed | `open_message` / `toggle_star` write locally and set `messages.flags_dirty`; `sync_now` pushes the queue before fetching, so the server cannot overwrite a local change |
| F9 | Sent mail arrived with no body | ✅ fixed | `drop_content_tag` listed `html`/`body`/`meta`/`link`/`base`, whose **content** it drops — so any full HTML document (what a rich-text editor emits, and what most HTML mail is) sanitized to an empty string and the recipient got `(empty)`. They now fall through to the tag allow-list, which keeps the content. The same bug emptied HTML mail in the reader |
| F10 | Formatting was lost on send even when typed | ✅ fixed | Qt rich text encodes bold as `style="font-weight:700"`, and the outgoing sanitizer strips `style` by design. The `execCommand` editor emits `<b>`/`<i>`/`<u>` instead, which survive |
| F11 | Message list showed the wrong time | ✅ fixed | `short_date` formatted the sender's own offset and compared against UTC midnight; it converts to `chrono::Local` first, so times read as the local clock and today/yesterday flip at local midnight |
| F12 | Buttons, dialogs and menus looked like a different app | ✅ fixed | Everything the app draws now comes from the design system: `AppButton`, `AppTextField`, `AppComboBox`, `AppCheckBox`, `AppMenu`, plus a window `palette` for the style-drawn leftovers (ScrollBar, ToolTip, selection). `standardButtons` are gone — those are drawn by the Controls style and cannot match |
| F13 | From address let you send as any domain | ✅ fixed | Composer splits the account address: the local part is editable, the domain is fixed and labelled as such (another domain would fail SPF/DMARC anyway) |
| F14 | Segfault after clicking the same message repeatedly | ✅ fixed | Every reload did `ListModel.clear()` + `append()` per row. `ListModel.get()` hands out QObjects the model owns, so the reader pane — which held the selected message — was left dereferencing freed memory as soon as the next click cleared the model, and every delegate was destroyed and rebuilt underneath the mouse handler that triggered it. The feed is now plain JavaScript objects (snapshots that stay valid), list models are updated in place by `ModelSync.sync()` (remove, insert, move, and write only the roles that changed), the reader keys its reloads on the UID instead of object identity, and re-clicking the open message is a no-op |
| F15 | Window title bar stayed white on a dark desktop | ✅ fixed | Windows draws the caption bar outside the Qt scene and Qt does not set `DWMWA_USE_IMMERSIVE_DARK_MODE` from the system colour scheme — a stock Qt window reports the attribute as off on a fully dark desktop. `platform.rs` sets it on the app's own top-level windows (and again if the desktop scheme changes); on Linux the compositor already follows the preference, so it is a no-op |
| F16 | Manage Contacts row overflow / inaccessible action buttons & fixed modal dialogs without resizing or dragging | ✅ fixed | Built `components/AppDialog.qml` as a generic resizable and draggable dialog component (header dragging, bottom-right resize grip canvas, edge/corner resizing, host bounds clamping, session geometry memory) adopted across Contacts, Accounts, Folders, MoveTo, Settings, and AccountSetup. In Contacts, constrained the contact layout with `Layout.minimumWidth: 0` and `wrapMode: Text.WrapAtWordBoundaryOrAnywhere`, dynamically sizing delegates to fit multi-line content without clipping; pinned the seen badge, edit button, and delete button to the right; added keyboard navigation (arrow keys, Enter to edit, Delete to remove) and full Accessible attributes |
| F17 | Saving attachments always failed on Windows ("cannot create folder … os error 123") | ✅ fixed | Save/Folder dialogs return `file://` URLs, but Rust only stripped the `file://` prefix: `file:///C:/…` became `/C:/…`, which Windows rejects (OS error 123), and `%20`/`%23` stayed encoded. New shared `mailcore::paths::file_url_to_path` (percent-decodes, strips the stray slash before drive letters, handles host-form/UNC/localhost/plain paths) now backs save, save-all, open-temp and composer-send attachment paths; QML builds save-dialog URLs via `joinFileUrl` (`writableLocation` is a QUrl on Qt 6, plain path elsewhere) with `encodeURIComponent` filenames. Stage timings (`select`/`fetch`/total download) logged at info for slow-save diagnosis |
| F18 | German Gmail folders showed raw IMAP modified UTF-7 (`[Google Mail]/Entw&APw-rfe` instead of `Entwürfe`), misclassified roles | ✅ fixed | New std-only `mailcore::sync::imap::utf7` (decode/encode + `mailbox_for_wire`); LIST/LSUB names are decoded before store (role heuristics now see `Entwürfe` → Drafts), every SELECT/COPY/MOVE/APPEND/CREATE encodes back to wire form; migration v12 renames/merges existing raw rows (SCHEMA_VERSION 12) |
| F19 | Sent mail filed twice in Gmail Sent (recipient got one copy) | ✅ fixed | Gmail auto-files every accepted SMTP submission into Sent Mail, so our APPEND duplicated it. `SmtpSender::{save_sent_copy,save_sent_copy_via}` now skip the APPEND for Gmail/Googlemail hosts (host-based, no content inspection); already-duplicated copies must be deleted once by hand |
| F20 | Gmail sync felt hung: no per-folder progress, no duration, status not copyable; big quiet folders paid full SEARCH+FETCH every sync | ✅ fixed | `sync_folder_window` fast path: SELECT-proven-unchanged folders (same validity/modseq/uidnext/count, no VANISHED) skip SEARCH+flag refresh+FETCH — one SELECT per quiet folder; Trash seen-sweep still runs. `sync_account` takes a progress callback (`SyncProgress`), GUI reports "Syncing 3/15: …" live via `job_progress`; sync summaries carry "in 42s" durations; status line is a read-only selectable TextEdit (drag + Ctrl+C) |
| F21 | Every sync ran all four discovery passes (~30 round-trips) before the first folder — slowest part of a Gmail start, folder tree barely changes | ✅ fixed | Throttled discovery: one LIST pass every run, full discovery only when the tree disagrees with the cache (new labels appear immediately) or the 2h interval lapsed (backstop for LIST-hiding servers); full runs stamp `last_full_discovery_{account}` in settings; manual folder refresh stays always-full |
| F22 | Widget/notification clicks opened the app on the wrong account with several accounts configured | ✅ fixed | `mailapp --open <id\|email> [folder]`: queues a take-once jump (`pending_open_*` settings, folder defaults to the account's inbox); a live GUI picks it up within 2s via a poll timer, switches account, lands on the inbox and raises itself (pid-file handover in `<db-dir>/gui.pid`); otherwise the same process boots the GUI there directly. Notification `--exec` and popup item clicks pass the newest mail's account+folder; generic open buttons stay as-is |
| F23 | Android background mail checks unreliable: the 15-minute WorkManager worker barely ran while the phone slept | ✅ fixed | Doze and App Standby defer a battery-optimised app's jobs by hours, so Settings → Accounts & sync now shows a background-check block (Android only): battery exemption state with an "Allow background use" button (`REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`, `MainActivity` channel `mailclient/background_power`; also asked when background checks get enabled), the standby bucket when it limits checks, and the last run from `mailcore::sync::background::LastRun` (started/finished/skipped/new/errors; a run that never finished was stopped by Android). The tick itself is now inbox-only over the cached folder tree (`SyncScope::InboxOnly`: no LIST, no other folders; outbox and flag pushes still run) so it fits a short Doze maintenance window. The background check moved out of `sync/headless.rs` into `sync/background.rs` |
| F24 | WorkManager notifications still only arrived on unlock: Doze never runs jobs inside deep sleep, only in maintenance windows | ✅ fixed | New `background_scheduler` setting (`workmanager` default, so Qt and existing installs are unaffected — the Qt bridge only touches keys it displays): Android Settings offers an "On-time alarm" alternative (`android_alarm_manager_plus`, exact + wakeup + AllowWhileIdle, reboot-resilient) that fires in Doze and honours 5/10-minute intervals, running the same Rust inbox check and notification path. `MailState.rescheduleBackgroundSync` stops the inactive scheduler so both never run together; `SCHEDULE_EXACT_ALARM` is requested via an "Alarms & reminders" screen (status block shows the grant state with an "Allow exact alarms" button; ungranted still fires, just inexact) |
| F25 | Android new-mail notifications still missed, even in alarm mode; several mails at once never showed; resuming the app did not refresh | ✅ fixed | The alarm plugin fired on time but ran its Dart callback as a plain `JobIntentService` job that Doze deferred: the alarm is now native (`MailAlarm.kt`, self-rearming exact one-shot, re-armed on boot/app update) and enqueues the check as expedited WorkManager work on the same dispatcher. Several new mails were posted as a lone group summary, which Android often hides: now one notification (id 0) listing all unread inbox mail since the app was last open (`BackgroundReport.pending`, `bg_shown_uid_*` line), alerting only for new mail, updated silently or removed when that mail is read elsewhere, cleared on resume. Resume/pause mark the inbox cache seen (`background_mark_seen`), resume reloads the list and syncs unless auto-sync is off or a sync ran within a minute. Settings shows the last ten checks (`bg_run_history`: trigger, result, notification outcome) |
| F26 | Android background checks drained far more battery than other mail apps, and notified later | ✅ fixed | Every check booted a headless Flutter engine, opened fresh TCP/TLS connections, and on any inbox change refetched the flags of the whole 200-mail window. Now: the worker, the alarm and a new Push scheduler run natively (`MailCheckWorker.kt`, `MailAlarm.kt`, `MailPushService.kt`) and call the core over JNI (`mailffi/src/android.rs`, `jni` crate), no engine; the notification plan moved from Dart into `mailcore::sync::background::notify` so all three share it. Push is IMAP IDLE (`ImapSession::idle`, `mailcore::sync::push`): one connection per account waits on the inbox in a `specialUse` foreground service with a hideable minimum-importance "Mail monitor" notification, syncs over the same session when the server announces mail, holds a wake lock only while busy, and is kept alive by a 15-minute exact alarm that also retries failed accounts; network changes reconnect. Window syncs ask flags `CHANGEDSINCE` the stored modseq when CONDSTORE is on. New mail found while the app is open reloads the list instead of alerting |
| F27 | Different IMAP servers want different sync settings (a server sending IDLE "Still here" every few minutes costs battery in push mode) | ✅ fixed | Settings → Accounts & sync gains a "Settings for" picker in both frontends: every account can override the check interval, Sent copy and recipient suggestions, plus push and new-mail notifications where the frontend has them (Flutter; push is Android-only); "Default (…)" inherits the app-wide value. Stored in the new `account_settings` table (schema v17) and resolved only through `store::account_settings`, which the SMTP Sent copy, contact collection, background notification plan (`notify::plan_for`) and the IDLE monitor read. `sync::background::schedule` plans the Android mechanisms from it: push service for push accounts, one poller at the shortest remaining interval, each tick syncing only due accounts (`bg_checked_at_{id}`). The foreground auto-sync timer of both frontends follows the open account's interval |
| F28 | Push and polling woke the phone all night; a chatty IDLE server woke it more than needed | ✅ fixed | Per-account **quiet hours** (both frontends; on/off, from/to, default 00:00–07:00, off): `account_settings::quiet_hours` (keys in `account_settings`, no schema change), `schedule::plan` leaves quiet accounts out (no IDLE connection, no poller tick, nothing at all when every account is quiet) and names `replan_at`, for which `MailSchedule.kt` arms a wall-clock alarm (plus a `quiet-end` check of polled accounts; time-zone/clock changes replan). The foreground timer skips a quiet account only while the window is unfocused; manual and app-start syncs are never affected. Push rides on server heartbeats: an IDLE older than `push::PIGGYBACK_AFTER` refreshes on the next heartbeat, and a keep-alive finding a fresh heartbeat (`push::HEARTBEAT_FRESH`) leaves the IDLE alone. Heartbeats inside IDLE never take the wake lock |
| F28 | Android new-mail notifications could not be expanded per mail and had no way to mark mail read | ✅ fixed | `notify::plan` now plans one Android group per account: a summary (count, inbox lines, rings only for new mail) plus one child per mail for the newest eight (sender, subject, cached snippet in the expanded view, arrival time), posted and cancelled by tag. The host passes what is on screen as tag → signature, so read mail drops out quietly and a swiped-away mail only returns when new. Children carry "Mark read", summaries "Mark all read": `MailActionReceiver` marks the cache (`notify::mark_read`, `flags_dirty`), re-plans, and `MailFlagWorker` pushes over a fresh connection (`headless::push_flags`) once online. Lock screens get a redacted public version |

### 8a. Static-review fixes

Found by code review rather than reported; all closed.

| Flaw | Resolution |
|---|---|
| A send refused as busy stayed queued and went out with the next sync | Busy is checked before the outbox row is written; failures before submit discard the row's MIME |
| GUI and `--sync-once` could submit the same outbox row | Atomic `queue::claim`, rows born claimed, stale-only crash requeue (see §5a) |
| Editing an account's address created a duplicate or overwrote another account | The edit form sends its account id; the bridge updates that id and refuses an address another account uses |
| A job for a deleted account fell back to the first remaining account | Jobs resolve their captured id strictly (`session::job_account`); deleting an account is refused while a job runs |
| Send/draft results could close or reuse a newer composition | Per-composition guards (see §5a) |
| Flag and settings write errors were swallowed behind a success message | `mark_read` / `toggle_star` / `open_message` and `SettingsBridge.save()` return the error to the status line |
| Narrow reader dead end, sticky selection after Back, crowded toolbar, cramped Settings, dialogs wider than the window | Reader falls back to the list, Back clears selection and the mark-read timer, compact toolbar + overflow menu, icon-rail Settings, dialogs and the recipient popup clamp to the window |
| Unscaled cue widths, wide HTML tables, unnamed glyph buttons, unreachable Contacts, mismatched Omarchy manifest | Scale-aware row cues, table cells shrink to the pane, `IconButton` names itself from its tooltip with a focus ring, Contacts in the toolbar/menu, manifest describes the panel's real clicks |

Reported working (keep while fixing): account setup + keyring, manual ⟳ sync, folder tree, send + Sent-copy, star/delete, remote-image blocking default.

## 9. Frontend feature parity (QML-first reference)

Rules: Qt/QML (desktop) and native Kotlin (Android) are the two active
frontends and match each other feature-for-feature, or record the
exception here; a feature that lands in one lands in the other. UI may
differ (touch vs desktop: bottom bars, fullscreen pages, long-press instead
of hover/menus); behaviour must not — it lives in `mailcore`, frontends
only translate. **Flutter is retired**: its column is kept for reference
and is not updated for new features (AGENTS.md §1). ✅ present, 🔄 partial,
❌ missing. Update this section when a step lands (see AGENTS.md §7.4);
the open Qt ↔ native differences are listed at the end.

### Shell, toolbar, status

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| 3-pane / responsive / 1-pane | ✅ | ✅ | ✅ | Flutter and native: <700 one pane, <1100 folders + list (reader takes the list's place), else three, divided by text/UI scale; Qt switches at 720/1100 window px. Three panes add the sidebar toggle |
| Resizable panes (drag dividers) | ✅ SplitView | ✅ `PaneDivider` | ✅ `PaneDivider` | Touch dividers keep a 24dp hit area; widths are not remembered across launches anywhere |
| Compose, Sync, search field, tools | ✅ | ✅ | ✅ | Narrow Flutter and native collapse the tools into an overflow menu |
| Folder-scoped search toggle | ✅ | ✅ | ✅ | Native: tools-menu checkbox |
| Busy indicator (a job is queued or running) | ✅ | ✅ | ✅ | Qt: core `busy` flag; Flutter: `_busyKinds`; native: header progress line fed by the core's in-flight job table (`mailffi::net`, sent with every job event) — never a Kotlin-side flag |
| Status line + details + copy | ✅ | ✅ | ✅ | Native StatusStrip shows while a job runs (live progress), on error, with outbox mail, and keeps a job's result readable briefly after it ends; tapping the line opens it in full with Copy |
| Outbox pill + dialog | ✅ | ✅ | ✅ page | Pill words from `outbox::status_json` (`label`); native opens a full page from the status-strip chip: rows with the core's state line and error, Sync now (only while something is retryable), forget a dead row after a confirm |
| Undo offer (snackbar/toast; Ctrl+Z where a keyboard exists) | ✅ | ✅ | ✅ | The bar lasts the core's `UNDO_GRACE_SECS` everywhere; native takes Ctrl+Z from a hardware keyboard outside the composer |
| Keyboard shortcuts | ✅ | ✅ | — | Touch: no shortcuts by design |
| Sync on start, account switch, resume | ✅ (start + switch; no resume on desktop) | ✅ | ✅ | Resume syncs only with auto-sync on and no sync of the account finished within the core's grace period (`mailcore::sync::resume`, 5 minutes); Flutter keeps its own one-minute gap. Start, switch, timer and manual syncs are never held back |
| Start view on narrow layouts (`start_view`: folder list or the last used account's inbox) | ✅ (narrow window, cold start) | — | ✅ (one pane, cold start) | Returning from the background keeps what was open |
| Auto-sync timer, quiet hours | ✅ | ✅ | ✅ | Native timer runs only while the app is in the foreground, so quiet hours (which gate unattended ticks) never apply there |

### Message list

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| Rows (avatar, unread, star, attach, snippet) | ✅ | ✅ | ✅ | Same arrangement everywhere: small avatar top left with the unread dot on its corner and the paperclip under it; sender + star, date on the right; subject with the ⋮ under the date; snippet (not in compact) |
| Sort (date/from/subject ±) | ✅ | ✅ | ✅ | Hidden during search everywhere |
| Quick filters (unread/starred/attach + dates + custom range) | ✅ | ✅ | ✅ | AND-combined; client-side over loaded rows + hits |
| Full query syntax (`is:`, `has:`, `after:`…) | ✅ | ✅ | ✅ | Core parses everywhere; the help text is the core's (Qt tooltip; Flutter and native a help button in the empty search field) |
| Selection + bulk bar (read/star/archive/move/trash/purge) | ✅ | ✅ | ✅ | Native bar docks at bottom; purge always confirms |
| Row menu (read/star/archive/move/trash/similar/eml) | ✅ | ✅ | ✅ | Touch: ⋮ on the subject line under the date (tap opens, long-press selects); Qt and Flutter desktop add right-click. A search hit acts in its own folder; purge always asks. Whether a delete asks first is `mailcore::undo::delete_prompt` everywhere: it asks when it destroys (any target folder would; an unknown folder counts), for every bulk delete, and otherwise per `confirm_delete` |
| Jump top/bottom buttons | ✅ | ✅ | ✅ | |
| Pull-to-refresh scope | — (toolbar syncs the account) | — (toolbar syncs the account) | ✅ folder-only inside a folder, account-wide in search | Native-only gesture; desktop has no pull |
| List scroll memory | ✅ (per-folder, UID-anchored) | ✅ (per-folder PageStorageKey) | ✅ (per-folder index) | Native drifts when new mail arrives mid-read; Qt's UID anchor does not |
| Load-older footer (Cached N [of M] / All loaded) | ✅ | ✅ | ✅ | |
| Find-similar mode + chip | ✅ | ✅ | ✅ | From the reader and the row menu; dismissable "Similar to: …" bar on native. Hits re-read after row, reader, undo and job changes; three panes keep the reader open |
| Drafts rows open the composer | ✅ | ✅ | ✅ | Native: any row whose folder has the `drafts` role, search hits included |
| List density (comfortable/compact) | ✅ | ✅ | ✅ | Compact drops the snippet line and tightens the rows |
| Swipe actions, mark-all-read | ❌ | ❌ | ❌ | None anywhere; not planned |
| Save as .eml | ✅ | ✅ | ✅ | Native via reader menu |

### Search

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| Short input = row filter, 3+ = FTS + server backfill | ✅ | ✅ | ✅ | Core `search::plan`; same thresholds. 1–2 letters filter the open folder in place ("X of Y shown", sort and footer stay); the local index answers every keystroke, only the server backfill waits out the debounce |
| Folder-scoped vs account-wide | ✅ | ✅ | ✅ | A folder-scoped search follows a folder change; a scope toggle or folder change starts a fresh server backfill. An account switch re-runs the search in Qt, clears it on native |
| Hits grouped under folder section headers | ✅ | ✅ | ✅ | Account-wide + similar only; folder-scoped stays flat everywhere |
| Jump to hit (opens in its folder) | ✅ | ✅ | ✅ | Native opens hit directly |

### Reader

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| Header (subject/sender/date/To, expandable) | ✅ | ✅ | ✅ | |
| Reply-To warning, link examine, headers view | ✅ | ✅ | ✅ | |
| Attachments (open/save/save-all) | ✅ | ✅ | ✅ | Native via SAF + FileProvider. Inline images sync never kept show a notice with Download in both readers (`missing_inline_images`) |
| Event card (ICS) | ✅ | ✅ | ✅ | Replies, counter-proposals and updates carry a notice line (`Jane accepted`, `Updated invitation`) in Qt and native; Flutter is retired |
| Delivery report card (bounce) | ✅ | — | ✅ | `multipart/report` delivery status: outcome, per-recipient reason in plain words plus the server text, original subject; **Edit & resend** opens the cached Sent original (found by Message-ID) as a new draft to the failed recipients, with its files. A successful delivery confirmation shows on the same card as **Delivered**; one that only got as far as a server without DSN (`relayed`) as **Delivery not confirmed** |
| Read receipt card | ✅ | — | ✅ | `message/disposition-notification` (RFC 8098): who, and whether the mail was opened, deleted unread or only received; **Open sent mail** jumps to the original (by `Original-Message-ID`, also offered on delivery reports). Receipts are only shown, never sent: a request in incoming mail is ignored |
| Attached mail card (.eml) | ✅ | — | ✅ | `message/rfc822` attachments: subject, sender, date, snippet; **Show message** expands the plain-text body; Open / Save .eml |
| Contact card (vCard) | ✅ | — | ✅ | `.vcf` attachments as a card (name + badge, title/org, e-mails, phones, address, URL; Open in Contacts / Save .vcf). Small `.vcf` bytes are kept at sync; an older uncached one shows a Download. Flutter is retired |
| Remote-image block + show-once | ✅ | ✅ | ✅ | |
| Original/darkened colours, zoom | ✅ | ✅ | ✅ | Reader text size factor is `settings::reader_text_scale`: Qt scales the HTML document's base size by it (sizes a mail sets stay its own), native uses it as WebView text zoom with pinch zoom on top |
| Fullscreen reader | ✅ | ✅ | ✅ | Toggle in the reader bar; hides the shell bars and the other panes, back leaves it first. Native also hides the Android system bars (swipe shows them briefly), so it gains room on a phone too |
| Reply / Reply-all / Forward | ✅ | ✅ | ✅ | Recipients, subject, quote and signature from `mailcore::compose::answer` everywhere |
| Forward keeps the attachments | ✅ | ❌ | ✅ | `mailcore::compose::forward` downloads files not cached yet, stages them for the composer and names any that could not be fetched. Flutter is retired |
| Archive / Move / Delete / Star | ✅ | ✅ | ✅ | |
| Prev/next message | ✅ keys | ❌ | ❌ | Qt: Up/Down step through the list and open the message; no button anywhere |

### Composer

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| Read receipt / delivery confirmation per mail | ✅ | — | ✅ | Two toggles in the composer (Qt: icon buttons beside Send; native: chips under Subject), starting from the settings `request_mdn` / `request_dsn`. Read receipt = `Disposition-Notification-To`; delivery confirmation = SMTP DSN `NOTIFY=SUCCESS,FAILURE,DELAY` (RFC 3461) over lettre's low-level connection, sent plain with a `sent, but …` note when the server offers no DSN. Each account remembers what its SMTP server offered (`accounts.smtp_dsn`, forgotten when host or port change): where it lacked DSN, delivery starts off and turning it on shows a warning; asking anyway re-checks. The DSN flag is kept on the outbox row for retries. A reopened draft starts from the settings again |
| Full composer (To/Cc/Bcc, editor, attach, drafts, send) | ✅ dialog | ✅ page | ✅ page | Touch composers are full pages on every width (keyboard). Locked From domain, contact autocomplete, Reply-To, reply-to-mismatch notice, server-draft notice, delete draft, dirty guard. Flutter carries the quote as a card beside its text box; Qt and native edit it inline in the body |
| Editor | ✅ WYSIWYG HTML + source | ✅ Markdown + preview | ✅ WYSIWYG HTML + source | Qt and native edit HTML in a web view (`execCommand`: bold, italic, underline, list, quote, link, clear, inline image; toolbar lights up at the caret). Native's page is `mailcore::compose::editor::document`; Qt still builds its own (see SHARED-CORE.md). With send format "auto" every frontend sends plain text when nothing is formatted (the sender's `needs_html_formatting`); both composers say which as you type (`compose::editor::send_format_note`) |
| Inline images, attachments | ✅ (+ drop) | ✅ (+ desktop drop) | ✅ | Native copies picked `content://` files into app cache, the core reads paths at send time. A reopened draft fetches missing bytes first and re-attaches its files in both (`compose::stage_draft_files`); a failed draft save reopens the native composer with the text |
| Send failure after the composer closed | ✅ reopens with the text | 🔄 status line | ✅ reopens with the text | SMTP runs after close. Qt and native keep the composition until SMTP accepts it and reopen it with the reason; new compositions wait meanwhile. The core drops the MIME on failure, so the retry cannot send twice. Flutter reports a late failure on the status strip only |

### Folders

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| Sidebar tree + unread pills + account switch | ✅ | ✅ | ✅ | Collapsible subfolders everywhere; known folders and inbox children always visible (`FolderRole::always_visible`). Qt shows total and unread side by side, native one or the other |
| Manager (create, refresh, hide) | ✅ | ✅ | ✅ | Hide is display-only everywhere (no IMAP unsubscribe) |
| Move picker | ✅ | ✅ | ✅ | |
| Rename / delete / empty folder | ❌ | ❌ | ❌ | None anywhere |

### Accounts & setup

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| List (use/edit/remove + confirm) | ✅ | ✅ | ✅ | |
| Setup form (identity, IMAP+SMTP, guess, port-follow) | ✅ | ✅ | ✅ | Security values from the core's `security_choices`, labelled in the same words, with the core's plaintext warning under the picker. The address guess never replaces a field the user typed |
| Pre-save connection test | ❌ | ❌ | ✅ | Native-only so far; promote to Qt/Flutter on demand |
| OAuth | ❌ | ❌ | ❌ | None anywhere |

### Contacts

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| Manager (alias, search, remove, cleanup review) | ✅ | ✅ | ✅ page | Native: shell page; alias edit in a small dialog, remove and bulk cleanup removal confirmed |

### Settings

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| Full settings (interface/mailbox/reading/composing/sync/maintenance/about) | ✅ dialog | ✅ page | ✅ page | Touch: full page, section rail where there is room, tabs (native) or a dropdown (Flutter) on a phone; draft + Save writes only changed keys. Native asks before discarding unsaved changes and applies the interface scale to every dp |
| Per-account overrides, About + server capabilities | ✅ | ✅ | ✅ | |

### Background & notifications (Android)

| Feature | Qt | Flutter | Native | Notes |
|---|---|---|---|---|
| Poll / push / alarm schedulers, quiet hours | — | ✅ | ✅ | Shared `mailcore::sync::background`; desktop uses poll timer. Both re-plan on start, after account changes and after a settings save (plus boot/update/clock) |
| Notification permission prompt (Android 13+) | — | ✅ | ✅ | Asked when the core's plan says something checks in the background (`any`, from `BackgroundPlan::view`) |
| Clear notifications on resume; no alert while open | — | ✅ | ✅ | Open app: a background check refreshes the list instead of alerting |
| Background status (permissions, battery, run history, heartbeat warning, test notification) | — | ✅ | ✅ | Run lines, standby-bucket name and heartbeat wording from `mailcore::sync::background::describe` on native; Flutter still words them in Dart (SHARED-CORE.md) |
| Grouped notifications + Mark read buttons | — | ✅ | ✅ | Same native path (`MailAlarm`, `MailPushService`) |
| Launcher shortcuts (long-press the app icon: Compose, each account's inbox) | — (`mailapp --open` serves widgets/notifications) | — | ✅ | `MailShortcuts.kt`, rebuilt on every account-list load; icons are the core's avatar initials/colour; a home-screen copy of a removed account's inbox is disabled. Taps reuse the notification-open path (`compose`, `inbox:<id>` payloads) |

### Open Qt ↔ native gaps

From a code audit of both frontends. Fix in `mailcore` where the gap is a
decision both should share; remove the row when closed (numbers stay, so
a closed gap leaves a hole). G1–G14 are closed; what is left are the
deliberate or minor differences below.

| # | Gap | Side | Kind |
|---|---|---|---|
| G15 | .eml export on native downloads on a throwaway runtime outside the job queue, with no progress line; Qt refuses Send and Save draft while any job (even a sync) runs, native only while that same job runs; an account switch re-runs an active search in Qt, clears it on native | both | minor |
