# PROJECT.md — Mailclient

## 1. Goal

A **full-featured, modern, responsive desktop mail client** for **Omarchy Linux** first (Windows later):

- Multiple IMAP/SMTP accounts, full folder trees, background sync (IDLE + polling).
- Send/receive **text + HTML** mail with a nice composer (rich-text editor, attachments, drafts).
- Fast local SQLite cache + full-text search, offline-first.
- Clean, modern QML UI: account/folder sidebar, message list, HTML reader, composer, search, contacts autocomplete.
- Extensible sync layer so future protocols (POP3, JMAP, EWS/Graph) can plug in.

## 2. Architecture

```text
crates/mailapp/qml/ ──QtQuick UI──▶  crates/mailapp (cxx-qt bridge, Qt models) ─▶ crates/mailcore (db/store/sync/queue)
Bay                                               ▲
                                  SQLite (~/.local/share/mailclient/mailclient.sqlite)
```

- `crates/mailcore`: pure Rust. Modules: `error`, `models`, `db/{mod,schema,migrations}`, `store/{accounts,folders,messages,queue,contacts}`, `sync/{traits,imap,sender}`, `search`.
- `crates/mailapp`: `cxx-qt` QObject bridge (`Bridge`, `SettingsBridge`, `AccountListModel`, `FolderTreeModel`, `MessageListModel`, composer controller) + `main.rs` loading `Main.qml` (embedded `Mailclient` module, filesystem override via `MAILCLIENT_QML_DIR`).
- `crates/mailapp/qml/`: `Main.qml`, `Sidebar.qml`, `MessageList.qml`, `MessageView.qml`, `Composer.qml`, `AccountSetup.qml`, `Settings.qml`, `components/*`.
- `scripts/`: `install-local.sh`, `qt-env.sh`, `smoke.sh`. Output bundle: `dist/mailclient/`.

See `AGENT.md` for agent rules, dependency policy, and Definition of Done.

## 3. SQLite Schema (v4, see `crates/mailcore/src/db/schema.sql`)

| Table | Purpose | Key columns |
|---|---|---|
| `schema_meta` | migration version | `key`, `value` |
| `accounts` | per-account IMAP/SMTP config, **no passwords** | `id`, `name`, `email_address`, `imap_host/port/security/username`, `smtp_host/port/security/username`, `auth_vault_key`, `check_interval_secs`, `created_at`, `updated_at` |
| `folders` | IMAP folder tree per account | `id`, `account_id→accounts`, `path`, `delimiter`, `role` (inbox/sent/drafts/trash/junk/archive/custom), `uid_validity`, `uid_next`, `subscribed`, `last_sync_at` |
| `messages` | cached headers + bodies | `id`, `account_id`, `folder_id→folders`, `uid`, `message_id_header`, `thread_id`, `subject`, `from_addr`, `to_addrs/cc/bcc/reply_to` (JSON), `date`, `snippet`, `body_text`, `body_html`, `flags` (`is_read/is_starred/is_draft/has_attachments`, `keywords` JSON), `size`, `downloaded_full`, UNIQUE `(account_id, folder_id, uid)` |
| `messages_fts` | FTS5 full-text index | `message_id→messages`, `subject`, `from_addr`, `body_text` |
| `attachments` | attachment names/sizes synced, bytes on explicit request only (SQLite BLOB cache, v4) | `id`, `message_id→messages`, `filename`, `mime_type`, `size`, `content_id`, `data` (BLOB, `NULL` until downloaded), `is_inline`, `storage_path` (legacy, unused) |
| `contacts` | autocomplete (built from mail) | `address` PK, `name`, `times_seen`, `last_seen_at` |
| `send_queue` | outbox for reliable sending | `id`, `account_id`, `message_id→messages`, `status` (queued/sending/sent/failed), `last_error`, `retries`, `created_at`, `updated_at` |
| `settings` | user preferences (v2) | `key` PK, `value` (`sent_copy_enabled=1`, `load_remote_images=0`, `auto_mark_read=1`, `mark_read_delay_secs=0`) |

Secrets live in the OS keyring keyed by `accounts.auth_vault_key`, never in SQLite.

## 4. Where We Stand (update as we go)

| # | Milestone | Status |
|---|---|---|
| 0 | Repo scaffold: workspace, `mailcore` schema + CRUD, `mailapp` cxx-qt skeleton, QML shell, `./dev.sh`/`./build.sh`/`scripts/install-local.sh`, `dist/` bundle | ✅ done |
| 1 | Real IMAP sync + app wiring: account setup (keyring), LIST/SELECT/FETCH, UIDVALIDITY handling, flag push/delete, send + Sent-copy, live folder/message feeds in QML | ✅ done (verified live against test account) |
| 2 | Composer polish: drafts, attachments, full rich-text editor (toolbar wraps selection today) | 🔶 partial (rich HTML compose + source view + Cc/Bcc + auto send-format + attachments done; drafts still M2) |
| 3 | Reader/search: FTS search UI, remote-image handling polish | 🔶 partial (safe sanitized HTML reader + remote-block banner + show-once done; FTS UI still M3) |
| 4 | Contacts, threading, notifications, settings UI extras | ⬜ planned |
| 5 | Polish: background IDLE/polling sync, offline/error states, onboarding, `.desktop`/icons, Windows feasibility | ⬜ planned (sync is manual ⟳ for now; IDLE not yet) |

Current state detail:
- `mailcore`: SQLite schema v4 (incl. FTS5 + `settings` table with migration, `messages.flags_dirty` v3, `attachments.data` BLOB + `is_inline` v4), typed stores (accounts/folders/messages/queue/contacts/settings incl. `compose_send_format`), `html` sanitizer (std-only tokenize→clean→serialise, remote/private-host gating, entity-aware), IMAP sync (SPECIAL-USE role mapping, windowed UID FETCH + MIME parsing — INBOX newest 200, others newest 50 auto / 200 on open — UIDVALIDITY resync, expunge, flag refresh/push, server-side delete, Sent-copy APPEND, attachment names/sizes extracted with 25 MiB/file + 50-file caps — bytes never auto-download, only `fetch_attachments` on explicit Open/Save spends bandwidth), SMTP send with `SendPolicy` + `SendFormat` (auto/plain/multipart/html, resilient fallback to auto; Auto sends text/plain unless the body carries real formatting, with an optional plain twin via `compose_include_plain`; Cc + Bcc, sanitized outgoing, `multipart/mixed` file attachments with extension-guessed MIME), keyring auth, safe JSON feeds (`body_text`/`body_html` sanitized/`is_html`/`has_remote_images` + legacy `body`, plus on-demand `message_html` for Show-once; message rows carry `has_attachments` + attachment metadata, never bytes). 51 unit tests green.
- `mailapp`: `Bridge` (accounts, selective sync + per-folder `sync_folder_now` + `load_older_messages` paging, folder LIST refresh + `subscribed` visibility, select/read/star/delete/send/archive/move with Cc/Bcc + format-aware bodies + composer file attachments, on-demand `message_html`, attachment open/save/save-all to disk, paged JSON feeds) + `SettingsBridge` (`sent_copy_enabled`, `load_remote_images`, `compose_send_format`, `compose_include_plain`, `auto_mark_read`, `mark_read_delay_secs`), embedded `Mailclient` QML module with filesystem override.
- `qml`: live 3-pane UI — real folders/messages, working sync/send/reply/forward/star/delete/archive, account setup with ports+encryption, settings dialog (incl. send-format picker), IMAP folder manager (LIST refresh, show/hide per folder, create folders, cached/unread counts). Reader: PlainText for plain (no more HTML-code display), sanitized WebEngine + blocked-images banner + working show-once (inline `cid:`/`data:` always load, remote gated + re-sanitized on demand), attachment bar with Open (downloads-if-needed to a temp copy for the system viewer) + per-file Save + Save-all (bytes download on first request, then cache as SQLite BLOBs → disk via save dialogs). `⋯` menu holds Reply-all/Archive/Move plus a Headers dialog (From/To/Cc/Date/Subject/Message-ID/Reply-To). List shows 📎 for mails with files and pages newest-first with a "Show older messages" button (one 200-mail server batch per press). Startup auto-sync, folder-open fill, post-send Sent refresh. Composer: compact headers (From/To/Subject + collapsible Cc/Bcc toggles beside To), WYSIWYG + HTML-source toggle, list/quote/link/clear, Cc/Bcc wired, send-format Auto (plain unless formatted, optional plain twin), file attachments (picker chips, sent as `multipart/mixed`), reply/forward quote from `body_text`. No mocks remain (search box + drafts still point at M2/M3).
- Verified live: 5 folders mapped, messages synced, test mail delivered + filed to Sent.
- QML is a responsive 3-pane shell (sidebar / list / reader + composer dialog + account setup dialog) with mock data so `qml6 qml/Main.qml` runs without Rust.

## 5. Sync Strategy (when / what / scaling)

Manual ⟳ plus auto-refresh on startup, folder open, and after send.

- **When**: cache shows instantly (offline-first); then auto-sync on startup
  (deferred past first paint), on every folder open (that folder only), and
  best-effort Sent refresh after each send. Read/star stay local + queued
  (`flags_dirty`) and push on the next sync; delete/purge hit IMAP at once.
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
  folders never auto-sync all mail, they fill (newest 200) when opened via
  `sync_folder_now`. Delete means Trash, except spam (destroyed outright,
  junk never touches Trash) and Trash itself (deleting there is permanent);
   one-click archive moves to Archive (auto-created server-side when missing).
   Expunge diffing is always full (local, no network);
   UIDVALIDITY resync on change. Attachment bytes are never fetched during
   sync — only names/sizes land in SQLite, so ⟳ stays cheap no matter how
   large the files are. A file downloads exactly once, on explicit user
   request (Download / Save / Save-all in the reader), then caches as a BLOB
   for offline use; resyncs never wipe downloaded bytes.
- **Scaling (massive mailboxes)**: per-folder network is bounded by the window
  (one SEARCH + ≤200 flag FETCH + ≤200 RFC822 FETCH), not by mailbox size.
  Planned next: larger chunks → CONDSTORE/QRESYNC deltas → IDLE push + polling.

## 6. Build / Run / Install

```sh
./dev.sh                # debug build + run (uses ./crates/mailapp/qml live)
/build.sh              # release build → dist/mailclient/{bin/mailapp,qml/,resources/}
/scripts/install-local.sh  # copy bundle to ~/.local/{bin,share/mailclient} + install .desktop
cargo test -p mailcore      # backend unit tests (SQLite in-memory)
qmllint crates/mailapp/qml/*.qml crates/mailapp/qml/components/*.qml  # QML lint (uses /usr/lib/qt6/bin when on PATH)
```

DB location: `~/.local/share/mailclient/mailclient.sqlite` (override `MAILCLIENT_DB=/tmp/x.sqlite` for tests/dev).

UI iteration: `./dev.sh` runs the app against live `crates/mailapp/qml/` (embedded module is the fallback). Every pane now does `import Mailclient` for the `Theme` singleton and the Rust QObjects, so no component previews standalone under `qml6` — iterate through `dev.sh`, which rebuilds and re-embeds on each run. Static checking is `qmllint` (its "Member not found on type Theme" noise is only the module not being importable outside the binary; the `Quick.layout-positioning` warnings are real).

## 7. Roadmap Notes

- Sync engine behind `SyncProvider` trait; IMAP first, JMAP/POP3 later without touching UI.
- HTML compose editing: `TextArea` rich-text now, consider WebEngine-based editor in M2.
- Windows: keep all paths via `directories`, no Linux-only calls outside `mailapp` platform shim.

## 8. Known Flaws & Repair List (user-reported)

F1–F15 below; all currently closed.

| # | Flaw | Status | Resolution |
|---|---|---|---|
| F1 | Cannot select other mails in the list (selection stuck / jumps back) | ✅ fixed | Selection is a UID, not a row index: clicks report `messageSelected(uid)`, the highlight derives from `currentUid`, and `onCurrentIndexChanged` no longer re-emits selection. Feed rebuilds can no longer move it. `open_message` also stopped opening an IMAP connection per click (see F8) |
| F2 | No body shown in reader (empty pane) | ✅ fixed | Plain bodies render in a `Flickable` + sized `TextEdit`; the old `ScrollView` + unsized `TextEdit` had no height and drew nothing |
| F3 | Composer is not WYSIWYG (Bold etc. don't reflect visually) | ✅ fixed | `components/EditorFrame.qml`: a `contentEditable` WebEngine document driven by `execCommand`, so text changes visibly and the toolbar buttons light up from `queryCommandState`. QML's rich-text `TextArea` has no selection-formatting API, which is why the old toolbar could only insert literal tags |
| F4 | Composer fields are placeholder-only, no labels | ✅ fixed | Shared `components/FormField.qml` labels From/To/Cc/Subject (and every dialog field) |
| F5 | Cancel loses the composed entry without asking | ✅ fixed | `dirty` tracking on all fields + a "Discard draft?" confirm; prefilled reply/forward quotes do not count as unsaved work |
| F6 | Overall UI looks sterile, flawed vs modern clients | ✅ fixed | `qml/Theme.qml` design-token singleton (light/dark, spacing, radius, type) + full redesign: list delegates with unread dot, hover, accent bar and inline star; reader header block; sidebar account chip and unread pills; themed dialogs. `main.rs` pins the Basic Controls style so Windows and Linux render identically instead of falling back to the native Windows style |
| F7 | Cannot manage accounts — a mistyped account can only be added, never removed or corrected | ✅ fixed | New `qml/Accounts.qml` manager (list, switch, edit, remove with confirm) on `Bridge.accounts_json` / `select_account` / `delete_account` / `account_form`. Editing keeps the stored password when the field is left blank; removal also drops the keyring secret |
| F8 | Clicking a message froze the UI and could revert read state | ✅ fixed | `open_message` / `toggle_star` write locally and set `messages.flags_dirty` (schema v3); `sync_now` pushes the queue before fetching, so the server cannot overwrite a local change |
| F9 | Sent mail arrived with no body | ✅ fixed | `drop_content_tag` listed `html`/`body`/`meta`/`link`/`base`, whose **content** it drops — so any full HTML document (what a rich-text editor emits, and what most HTML mail is) sanitized to an empty string and the recipient got `(empty)`. They now fall through to the tag allow-list, which keeps the content. The same bug emptied HTML mail in the reader |
| F10 | Formatting was lost on send even when typed | ✅ fixed | Qt rich text encodes bold as `style="font-weight:700"`, and the outgoing sanitizer strips `style` by design. The `execCommand` editor emits `<b>`/`<i>`/`<u>` instead, which survive |
| F11 | Message list showed the wrong time | ✅ fixed | `short_date` formatted the sender's own offset and compared against UTC midnight; it converts to `chrono::Local` first, so times read as the local clock and today/yesterday flip at local midnight |
| F12 | Buttons, dialogs and menus looked like a different app | ✅ fixed | Everything the app draws now comes from the design system: `AppButton`, `AppTextField`, `AppComboBox`, `AppCheckBox`, `AppMenu`, plus a window `palette` for the style-drawn leftovers (ScrollBar, ToolTip, selection). `standardButtons` are gone — those are drawn by the Controls style and cannot match |
| F13 | From address let you send as any domain | ✅ fixed | Composer splits the account address: the local part is editable, the domain is fixed and labelled as such (another domain would fail SPF/DMARC anyway) |
| F14 | Segfault after clicking the same message repeatedly | ✅ fixed | Every reload did `ListModel.clear()` + `append()` per row. `ListModel.get()` hands out QObjects the model owns, so the reader pane — which held the selected message — was left dereferencing freed memory as soon as the next click cleared the model, and every delegate was destroyed and rebuilt underneath the mouse handler that triggered it. The feed is now plain JavaScript objects (snapshots that stay valid), list models are updated in place by `ModelSync.sync()` (remove, insert, move, and write only the roles that changed), the reader keys its reloads on the UID instead of object identity, and re-clicking the open message is a no-op |
| F15 | Window title bar stayed white on a dark desktop | ✅ fixed | Windows draws the caption bar outside the Qt scene and Qt does not set `DWMWA_USE_IMMERSIVE_DARK_MODE` from the system colour scheme — a stock Qt window reports the attribute as off on a fully dark desktop. `platform.rs` sets it on the app's own top-level windows (and again if the desktop scheme changes); on Linux the compositor already follows the preference, so it is a no-op |


Reported working (keep while fixing): account setup + keyring, manual ⟳ sync, folder tree, send + Sent-copy, star/delete, remote-image blocking default.
