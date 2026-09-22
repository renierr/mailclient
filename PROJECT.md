# PROJECT.md — Mailclient

## 1. Goal

A **full-featured, modern, responsive desktop mail client** for **Omarchy Linux** first (Windows later):

- Multiple IMAP/SMTP accounts, full folder trees, background sync (polling + instant navigation sync + Omarchy bar widget).
- Send/receive **text + HTML** mail with a nice composer (rich-text editor, attachments, drafts).
- Fast local SQLite cache + full-text search, offline-first.
- Clean, modern QML UI: account/folder sidebar, message list, HTML reader, composer, search, contacts autocomplete.
- Extensible sync layer so future protocols (POP3, JMAP, EWS/Graph) can plug in.

## 2. Architecture

```text
crates/mailapp/qml/ ──QtQuick UI──▶  crates/mailapp (cxx-qt bridge, Qt models) ─┐
                                                                               ├─▶ crates/mailcore (db/store/sync/queue)
flutter/lib/ ────────Flutter UI───▶  crates/mailffi (flutter_rust_bridge, FFI) ─┘
                                                  ▲
                                  SQLite (~/.local/share/mailclient/mailclient.sqlite)
```

Two frontends, one core. Neither is authoritative: behaviour lives in
`mailcore` so both inherit it, and each adapter crate only translates. On a
desktop with both installed they open the same database file, on purpose.

- `crates/mailcore`: pure Rust. Modules: `error`, `models`, `db/{mod,schema,migrations}`, `store/{accounts,folders,messages,queue,contacts}`, `sync/{traits,imap,sender}`, `search`.
- `crates/mailapp`: `cxx-qt` QObject bridge (`Bridge`, `SettingsBridge`, `AccountListModel`, `FolderTreeModel`, `MessageListModel`, composer controller) + `main.rs` loading `Main.qml` (embedded `Mailclient` module, filesystem override via `MAILCLIENT_QML_DIR`).
- `crates/mailapp/qml/`: `Main.qml`, `Sidebar.qml`, `MessageList.qml`, `MessageView.qml`, `Composer.qml`, `AccountSetup.qml`, `Settings.qml`, `components/*`.
- `crates/mailffi`: `cdylib` over `mailcore` for the Flutter frontend —
  `api/{init,events,accounts,folders,messages,mutate,sync,search,composer,attachments,contacts,settings}`
  plus `net` (the shared `mailclient-net` thread), `session` (IMAP pool) and
  `db` (per-thread handle). No mail logic, no Qt.
- `flutter/`: the Dart app — `src/ffi` (library loading + generated bindings),
  `src/models`, `src/state`, `src/ui/{shell,sidebar,message_list,reader,accounts}`.
  See `flutter/README.md` for the layering and its open questions.
- `scripts/`: `install-local.sh`, `qt-env.sh`, `smoke.sh`. Output bundle: `dist/mailclient/`.
- `flutter_rust_bridge.yaml` (repo root): FFI codegen config. At the root
  rather than in `flutter/` because the tool does not normalise a leading `..`
  on Windows.

See `AGENT.md` for agent rules, dependency policy, and Definition of Done.

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
| `send_queue` | outbox for reliable sending | `id`, `account_id`, `message_id→messages`, `status` (queued/sending/sent/failed), `last_error`, `retries`, `raw_mime` + `envelope_from` + `envelope_to` (the built message, so a send survives a crash), `created_at`, `updated_at` |
| `settings` | user preferences | `key` PK, `value`. Keys and defaults are defined in `store::settings`, which is where to look rather than here |

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
| 6 | Bar integration: shared `mailcore::sync::headless` (GUI + CLI same orchestration), `mailapp --sync-once/--status [--json]`, cross-process `.sync.lock`, `resources/omarchy/mailclient/` bar-widget plugin (status poll + sync timers, notify-on-rise, click-to-open) | ✅ done |
| 7 | Flutter frontend: `mailffi` cdylib (flutter_rust_bridge 2, in-process `dart:ffi`), Dart app in `flutter/` with responsive 3/2/1-pane shell, sidebar, list, reader, account setup; CMake wiring for Windows + Linux, Gradle wiring for Android | 🚧 scaffold complete, see below |

Milestone 7 detail — what works and what does not:
- **Works**: the whole read path and the local write path. Accounts
  (list/add/edit/delete, keyring), folders (tree, unread pills, subscription),
  messages (paged list, reader with sanitized HTML and the blocked-remote-images
  "show once", attachment bar), local flag writes with background push, queued
  sync / folder sync / load-older / folder refresh, queued delete / archive /
  move / purge, FTS search plus server backfill, contacts, settings. The Rust
  API for send, drafts and attachment download is implemented and tested for
  what it can be offline.
- **Not built yet in the UI**: composer, settings screen, search UI, contacts
  manager, multi-select and bulk actions, folder manager, About. Each of these
  has its `mailffi` call already; what is missing is the Dart screen.
- **Not verified against a live server**: nothing in the Flutter path has been
  run against a real mailbox yet. `cargo test`, `cargo clippy -D warnings`,
  `flutter analyze` and `flutter test` are clean, and `flutter build windows`
  produces a bundle with `mailffi.dll` in it, but a live run needs explicit
  per-run consent (`AGENT.md` §2).
- **Android does not compile**: `mailcore::auth` uses `keyring`
  unconditionally while `mailcore`'s manifest only depends on it for Linux,
  Windows and macOS. Android needs a Keystore-backed path in `mailcore` — a
  design decision, not a build fix. The Gradle/cargo-ndk wiring is in place so
  that work stays confined to `mailcore`.
- **Known duplication**: the IMAP session pool, the account-form handling and
  the composer form now exist in both `mailapp` and `mailffi`. They belong in
  `mailcore`; the list is in `flutter/README.md`.

Current state detail:
- `mailcore`: the SQLite schema of §3 (FTS5 index, `settings` key/value store, local-change queueing via `messages.flags_dirty`, attachment bytes cached as BLOBs), typed stores (accounts/folders/messages/queue/contacts/settings incl. `compose_send_format`), `html` sanitizer (std-only tokenize→clean→serialise, remote/private-host gating, entity-aware incl. `&nbsp;`), IMAP sync (SPECIAL-USE role mapping, windowed UID FETCH + MIME parsing incl. Reply-To capture — INBOX newest 200, others newest 50 auto / 200 on open — UIDVALIDITY resync, expunge, flag refresh/push, server-side delete, Sent-copy APPEND, attachment names/sizes extracted with 25 MiB/file + 50-file caps — bytes never auto-download, only `fetch_attachments` on explicit Open/Save spends bandwidth). The default Date list order is IMAP UID/delivery order, not the sender-controlled RFC 5322 `Date:` header. SMTP send with `SendPolicy` + `SendFormat` (auto/plain/multipart/html, resilient fallback to auto; Auto sends text/plain unless the body carries real formatting, with an optional plain twin via `compose_include_plain`; Cc + Bcc; blank/placeholder To sends `To: undisclosed-recipients:;` (or `To: <text>:;`) with the envelope from Cc/Bcc; sender display name from the composer or account default; EHLO uses the sender domain; sanitized outgoing, `multipart/mixed` file attachments with extension-guessed MIME), keyring auth, safe JSON feeds (`body_text`/`body_html` sanitized/`is_html`/`has_remote_images` + legacy `body`, plus on-demand `message_html` for Show-once; message rows carry `has_attachments` + attachment metadata, never bytes).
- `mailapp`: `Bridge` (accounts, persisted last-active account restored at startup, active-account-only sync on switch, selective sync + per-folder `sync_folder_now` + `load_older_messages` paging, folder LIST refresh + `subscribed` visibility, select/read/star/delete/send/archive/move with Cc/Bcc + format-aware bodies + composer file attachments, on-demand `message_html`, attachment open/save/save-all to disk, compact paged list feeds plus an on-demand full reader payload so folder navigation never sanitizes 200 bodies, persisted list sort + bulk mark/star/delete/archive/move/purge) + `SettingsBridge` (`sent_copy_enabled`, `load_remote_images`, `compose_send_format`, `compose_include_plain`, `auto_mark_read`, `mark_read_delay_secs`, `collect_sent_contacts`, `confirm_delete`, `list_density`, `reader_font_size`, `sync_interval_minutes`, `signature_enabled`/`signature_text`, `reply_below_quote`, `request_mdn`, `ui_scale`), `Bridge` app version/license (`app_version`/`app_license` from the package manifest) + per-account live IMAP CAPABILITY viewer (`refresh_server_capabilities` on the net thread, JSON via `job_finished`), embedded `Mailclient` QML module with filesystem override.
- `qml`: live 3-pane UI — real folders/messages, working sync/send/reply/forward/star/delete/archive, account setup with ports+encryption, Roundcube-style settings (Interface / Mailbox / Reading / Composing / Accounts & sync / About sections, scrollable, Cancel truly reverts) with interface scale, delete-confirm, list density, reader text size, auto-check interval, signature, reply above/below quote and read-receipt request, About showing the app version + short licence info alongside the per-account IMAP server capabilities (account picker + Refresh, pill list), IMAP folder manager (LIST refresh, show/hide per folder, create folders, cached/unread counts). Reader: PlainText for plain (no more HTML-code display), sanitized WebEngine + blocked-images banner + working show-once (inline `cid:`/`data:` always load, remote gated + re-sanitized on demand), attachment bar with Open (downloads-if-needed to a temp copy for the system viewer) + per-file Save + Save-all (bytes download on first request, then cache as SQLite BLOBs → disk via save dialogs). `⋯` menu holds Reply-all/Archive/Move plus a Headers dialog (From/To/Cc/Date/Subject/Message-ID/Reply-To). A differing Reply-To is shown inline in the reader and as a banner when replying, so answering visibly goes to the right address. List shows 📎 for mails with files and pages in sort order with a "Show older messages" button (one 200-mail server batch per press), plus Roundcube-style multi-select (header ☑ toggle reveals checkboxes, Ctrl/Shift range, select all/none/unread/starred/invert, bulk read/unread/star/archive/move/Trash with purge confirm) and sorting (Date/From/Subject, asc/desc, persisted). Startup auto-sync, folder-open fill, post-send refresh of Sent + viewed folder (composer validates + queues locally and closes at once; SMTP submit, Sent copy, draft removal and resync run in the background, failures report as `sent, but …` or reopen the composer with the text kept). Composer: compact headers (From with per-mail sender name + account default / To / Subject + collapsible Cc/Bcc toggles beside To, optional Reply-To behind a collapsed ↩ toggle beside From), WYSIWYG + HTML-source toggle, list/quote/link/clear, Cc/Bcc wired, send-format Auto (plain unless formatted, optional plain twin), file attachments (picker chips, sent as `multipart/mixed`), reply/forward quote in the received format (`>` citations for plain, blockquote for HTML). No mocks remain.
- Verified live: 5 folders mapped, messages synced, test mail delivered + filed to Sent.
- QML is a responsive 3-pane shell (sidebar / list / reader + composer dialog + account setup dialog). Every model it shows comes from the Rust bridge, so it needs the app to run — there is no mock-data path that renders it standalone.

## 5. Sync Strategy (when / what / scaling)

Manual ⟳ plus auto-refresh on startup and after send. Folder switches are
cache-only so they render immediately.

- **When**: cache shows instantly (offline-first); folder clicks only change
  the local feed. Auto-sync runs on startup (deferred past first paint), plus
  a best-effort Sent refresh after each send. The toolbar Sync refreshes the
  account explicitly. Read/star stay local + queued
  (`flags_dirty`) and push on the next sync; a quiet background job also
  pushes them seconds after every toggle (no busy latch, failures stay dirty),
  so quitting right after reading loses nothing. Delete/purge hit IMAP at once.
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

### Dropped Features (And Why)
- **IMAP IDLE (RFC 2177)**: Dropped (not needed). Periodic background polling + instant navigation sync on folder click + Omarchy bar widget (`mailclient.unread`) completely satisfy real-time mail needs with zero TCP connection lifecycle management.
- **Background System Daemon**: Dropped (not needed). The Omarchy bar widget already runs headless periodic syncs in the background, and the desktop app retries pending sends while open. An external system service/daemon is redundant.
- **UIDPLUS (RFC 4315)**: Partly adopted, for `UID EXPUNGE` only. It was dropped for discovery, and that still holds — standard folder refresh plus local UID diffing find new and moved messages without it. Deletion is different: flagging `\Deleted` and then issuing a mailbox-wide `EXPUNGE` also destroys whatever another client has flagged but not yet expunged, i.e. someone else's pending, still-undoable delete. Where the server advertises UIDPLUS we expunge by UID; where it does not, we fall back to the mailbox-wide form, which is why that hazard is a fallback and not the default.
- **Streaming Partial MIME Parts (RFC 3516 / BINARY)**: Dropped (not needed). Single-roundtrip full RFC 822 body fetch is faster and simpler for normal desktop workloads; attachments are capped at 25MB and downloaded on demand.
- **Optimistic Concurrency (`UNCHANGEDSINCE`)**: Dropped (not needed). Desktop user actions are authoritative; newest state reconciles on next fetch without conflict retry loops.

### Verification
- The whole suite runs offline in about a second and makes zero live network calls: `mailcore`'s tests drive an in-memory raw IMAP wire-protocol mock server and an in-memory SQLite, and `mailapp`'s cover the bridge's own logic. `cargo test --workspace` is the check; a test that dials out does not belong here (see AGENT.md).
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` are clean; both are part of finishing a change (see AGENT.md).
- Release bundle builds cleanly via `./build.sh` into `dist/mailclient/bin/mailapp`.

## 6. Build / Run / Install

```sh
./dev.sh                # debug build + run (uses ./crates/mailapp/qml live)
./build.sh              # release build → dist/mailclient/{bin/mailapp,qml/,resources/}
./scripts/install-local.sh  # copy bundle to ~/.local/{bin,share/mailclient} + install .desktop
cargo test -p mailcore      # backend unit tests (SQLite in-memory)
qmllint crates/mailapp/qml/*.qml crates/mailapp/qml/components/*.qml  # QML lint (uses /usr/lib/qt6/bin when on PATH)
```

DB location: `~/.local/share/mailclient/mailclient.sqlite` (override `MAILCLIENT_DB=/tmp/x.sqlite` for tests/dev).

UI iteration: `./dev.sh` runs the app against live `crates/mailapp/qml/` (embedded module is the fallback). Every pane now does `import Mailclient` for the `Theme` singleton and the Rust QObjects, so no component previews standalone under `qml6` — iterate through `dev.sh`, which rebuilds and re-embeds on each run. Static checking is `qmllint` (its "Member not found on type Theme" noise is only the module not being importable outside the binary; the `Quick.layout-positioning` warnings are real).

## 7. Roadmap Notes

- Sync engine behind `SyncProvider` trait; IMAP first, JMAP/POP3 later without touching UI.
- HTML compose editing: `TextArea` rich-text now, consider WebEngine-based editor in M2.
- Windows: keep all paths via `directories`, no Linux-only calls outside `mailapp` platform shim.
- Completed: CONDSTORE/QRESYNC delta sync, the `imap-next` tokio migration, capability guards, fallbacks, and trash seen sync — see §5a, which is the full record. IDLE and a separate background daemon are explicitly dropped; UIDPLUS is used for `UID EXPUNGE` and nothing else (§5a).

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

Reported working (keep while fixing): account setup + keyring, manual ⟳ sync, folder tree, send + Sent-copy, star/delete, remote-image blocking default.

