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
- `scripts/`: `build.sh`, `dev.sh`, `install-local.sh`. Output bundle: `dist/mailclient/`.

See `AGENT.md` for agent rules, dependency policy, and Definition of Done.

## 3. SQLite Schema (v1, see `crates/mailcore/src/db/schema.sql`)

| Table | Purpose | Key columns |
|---|---|---|
| `schema_meta` | migration version | `key`, `value` |
| `accounts` | per-account IMAP/SMTP config, **no passwords** | `id`, `name`, `email_address`, `imap_host/port/security/username`, `smtp_host/port/security/username`, `auth_vault_key`, `check_interval_secs`, `created_at`, `updated_at` |
| `folders` | IMAP folder tree per account | `id`, `account_id→accounts`, `path`, `delimiter`, `role` (inbox/sent/drafts/trash/junk/archive/custom), `uid_validity`, `uid_next`, `subscribed`, `last_sync_at` |
| `messages` | cached headers + bodies | `id`, `account_id`, `folder_id→folders`, `uid`, `message_id_header`, `thread_id`, `subject`, `from_addr`, `to_addrs/cc/bcc/reply_to` (JSON), `date`, `snippet`, `body_text`, `body_html`, `flags` (`is_read/is_starred/is_draft/has_attachments`, `keywords` JSON), `size`, `downloaded_full`, UNIQUE `(account_id, folder_id, uid)` |
| `messages_fts` | FTS5 full-text index | `message_id→messages`, `subject`, `from_addr`, `body_text` |
| `attachments` | attachment metadata (blobs on disk under `attachments/`) | `id`, `message_id→messages`, `filename`, `mime_type`, `size`, `content_id`, `storage_path` |
| `contacts` | autocomplete (built from mail) | `address` PK, `name`, `times_seen`, `last_seen_at` |
| `send_queue` | outbox for reliable sending | `id`, `account_id`, `message_id→messages`, `status` (queued/sending/sent/failed), `last_error`, `retries`, `created_at`, `updated_at` |
| `settings` | user preferences (v2) | `key` PK, `value` (`sent_copy_enabled=1`, `load_remote_images=0`) |

Secrets live in the OS keyring keyed by `accounts.auth_vault_key`, never in SQLite.

## 4. Where We Stand (update as we go)

| # | Milestone | Status |
|---|---|---|
| 0 | Repo scaffold: workspace, `mailcore` schema + CRUD, `mailapp` cxx-qt skeleton, QML shell, `scripts/{build,dev,install-local}.sh`, `dist/` bundle | ✅ done (this commit) |
| 1 | Real IMAP sync: LIST/SELECT/FETCH, UIDVALIDITY handling, IDLE + polling, flag push | 🟡 engine done, verified live; QML models + IDLE next (see below) |
| 2 | Send path: composer → `send_queue` → `lettre` SMTP, drafts, attachments | ⬜ planned |
| 3 | QML models live: folder tree, virtualised message list, WebEngine reader, search + FTS | ⬜ planned |
| 4 | Contacts, threading, notifications, keyring secrets, settings UI | ⬜ planned |
| 5 | Polish: offline/error states, onboarding, `.desktop`/icons, Windows feasibility | ⬜ planned |

Current state detail:
- `mailcore` compiles with rusqlite-backed `Db`, full v1 schema incl. FTS5, and tested CRUD for accounts/folders/messages/queue/contacts.
- **M1 sync engine works against a live server** (`crates/mailcore/examples/sync_test.rs` + `.env`): login, LIST with SPECIAL-USE role mapping (inbox/sent/drafts/trash/junk + custom IMAP folders kept), UID FETCH with MIME parsing into SQLite, UIDVALIDITY resync, expunge handling, flag refresh. Verified: 5 folders, 20 messages synced.
- SMTP send path implemented with an allowlist policy (`SendPolicy`: test sends only to explicitly allowlisted recipients; unset allowlist denies all); **verified live** against the test account. Sent-copy filing (`APPEND` to Sent, Thunderbird-style, default on via `sent_copy_enabled` setting) implemented; live verify pending.
- Still open in M1: Rust `QAbstractListModel`s (folder tree, message list) replacing QML mocks; IDLE/polling background sync; `push_flags` live test.
- `mailapp` is a cxx-qt skeleton bridge (`Bridge` QObject with `ping`, `accountCount`, `dbPath`) that boots `qml/Main.qml`; full list models land with the rest of M1.
- QML is a responsive 3-pane shell (sidebar / list / reader + composer dialog + account setup dialog) with mock data so `qml6 qml/Main.qml` runs without Rust.

## 5. Build / Run / Install

```sh
./scripts/dev.sh            # debug build + run (uses ./crates/mailapp/qml live)
./scripts/build.sh          # release build → dist/mailclient/{bin/mailapp,qml/,resources/}
./scripts/install-local.sh  # copy bundle to ~/.local/{bin,share/mailclient} + install .desktop
cargo test -p mailcore      # backend unit tests (SQLite in-memory)
qmllint crates/mailapp/qml/*.qml crates/mailapp/qml/components/*.qml  # QML lint (uses /usr/lib/qt6/bin when on PATH)
```

DB location: `~/.local/share/mailclient/mailclient.sqlite` (override `MAILCLIENT_DB=/tmp/x.sqlite` for tests/dev).

UI iteration: `./scripts/dev.sh` runs the app against live `crates/mailapp/qml/` (embedded module is the fallback). Note: files using `import Mailclient` (currently `Settings.qml`) only load inside the app, not under standalone `qml6` — preview other components individually with `qml6` instead.

## 6. Roadmap Notes

- Sync engine behind `SyncProvider` trait; IMAP first, JMAP/POP3 later without touching UI.
- HTML compose editing: `TextArea` rich-text now, consider WebEngine-based editor in M2.
- Windows: keep all paths via `directories`, no Linux-only calls outside `mailapp` platform shim.
