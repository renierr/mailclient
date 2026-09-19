# IMAP Sync Resilience & `imap-next` Migration

## Overview

This document describes the architectural changes, bug fixes, capability guards, and fallback mechanisms introduced during the migration to `imap-next` and the stabilization of IMAP synchronization in `mailclient`. It also documents what was deliberately left out and the rationale behind those decisions.

---

## 1. What Was Done

### 1.1 Architecture & Transport
- **`imap-next` (v0.3.4)**: Replaced legacy blocking `imap 2.4.1` with `imap-next` (built on `imap-codec 2.0.0-alpha.9` and `imap-types 2.0.0-alpha.7`), adopting a sans-I/O state machine over Tokio.
- **Async TLS**: Integrated `tokio-rustls 0.26` with `rustls-native-certs` and `webpki-roots` for robust cross-platform certificate validation.
- **Plaintext Guard**: Plaintext connections are refused unless the account configuration explicitly specifies `imap_security = "plain"` or `"none"`.

### 1.2 Capability Discovery & Negotiation
- **Multi-Source Capability Collection ([`capability`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L338))**:
  - Collects capabilities from `Data::Capability` responses, untagged `* OK [CAPABILITY ...]` status codes, and tagged `OK [CAPABILITY ...]` completion codes.
  - Fixes capability blindness where servers only advertise capabilities in status codes rather than dedicated capability data.
- **RFC 5161 `ENABLE` Guard ([`enable_extensions`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L368))**:
  - The client checks `has_capability("ENABLE")` before issuing the `ENABLE` command.
  - If `ENABLE` is not advertised, `ENABLE` is never sent.
  - If `CONDSTORE` is advertised without `ENABLE`, it is activated via `SELECT (CONDSTORE)` according to RFC 4551.

### 1.3 Resilient Fallbacks for Extended Capabilities
- **`SELECT` Fallback ([`select`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L433))**:
  - When `qresync_enabled` or `condstore_enabled` is active, the client attempts `SELECT` with `SelectParameter::QResync` or `SelectParameter::CondStore`.
  - If the server rejects the command (e.g. `BAD [CANNOT] parameter not supported`), the client catches the error, sets `qresync_enabled = false` and `condstore_enabled = false`, and automatically retries with standard RFC 3501 `SELECT`.
- **`CHANGEDSINCE` Fallback ([`uid_fetch_flags_changesince`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L538))**:
  - When querying flag updates using `FetchModifier::ChangedSince(modseq)`, if the server rejects the command with `BAD`, the client logs a warning, sets `condstore_enabled = false`, and retries with standard `UID FETCH (UID FLAGS)`.
- **`MOVE` Fallback ([`uid_move`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L747))**:
  - Checks `has_capability("MOVE")`. If absent, directly uses `COPY + STORE \Deleted + EXPUNGE`.
  - If `MOVE` is advertised but the `UID MOVE` command fails at runtime (e.g., `NO Move not allowed`), it catches the failure and executes the `COPY + STORE \Deleted + EXPUNGE` fallback sequence.
- **Local Expunge Diffing**:
  - Always runs local UID diffing (`*uid >= search_lo && !server_uids.contains(uid)`) against local SQLite records.
  - Ensures server-side deletions are cleaned up from SQLite at zero network cost, even on servers without QRESYNC/VANISHED support.

### 1.4 Composer & Sent Copy Delivery
- **Non-Blocking Sent Copy ([`save_sent_copy`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/sender.rs#L875))**:
  - Converted `save_sent_copy`, `flush_outbox`, and `send_raw` to fully `async` functions.
  - Removed `tokio::task::block_in_place`, which panicked on Tokio's `current_thread` runtime used in `mailclient-net`.
  - In [`composer.rs`](file:///home/cody/dev/rust/mailclient/crates/mailapp/src/bridge/composer.rs#L176), `save_sent_copy` is awaited directly, preventing the false delivery failure dialog and composer reopening.

### 1.5 Trash & Deletion Read Flag ("Always Seen")
- **Immediate Server Sync on Trash ([`trash_message`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L1216), [`move_to_folder`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L1275), & [`move_uids_to`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L1324))**:
  - Whenever a message is moved to Trash (via Delete button, Bulk Delete, or "Move to..." dialog), `UID STORE <uid> +FLAGS (\Seen)` is **immediately executed on the IMAP server** on the active connection *before* `UID MOVE` or `COPY + EXPUNGE` is executed.
  - This is done unconditionally (regardless of local `is_read` state) because local state might have been marked read while the server was still pending sync (see Flaw F8).
- **Enforced Seen State During Trash Sync ([`sync_folder_window`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L1434))**:
  - When the Trash folder is synced, all fetched messages and flag updates are forced to `is_read = true`.
  - If any message in Trash on the server is still unread, `sync_folder_window` immediately sends `UID STORE <uids> +FLAGS (\Seen)` to ensure the server-side Trash folder has 0 unread messages.
- **Feed Representation ([`feed.rs`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/feed.rs#L18))**:
  - `folders_json` reports `unread: 0` for Trash folders (no unread pills/badges in the sidebar).
  - `messages_json`, `messages_list_json_paged`, and `message_json` report `unread: false` for all messages residing in Trash.

### 1.6 Flag Sync Timing (Immediate vs Queued)
- **When Trashing / Deleting**: The `\Seen` flag is synced **immediately** to the IMAP server in the same job before moving.
- **When Opening / Reading Messages**: To prevent UI latency and freezes when clicking through messages rapidly (Flaw F8 in `PROJECT.md`), `open_message` updates `is_read = true` locally in SQLite immediately and queues the flag change (`flags_dirty = true`). The flag push to IMAP occurs on the next sync (automatic interval, folder switch, or manual refresh).
- **When Explicitly Toggling Read / Star**: Queued in SQLite and pushed during sync, or pushed immediately if an IMAP session is active.

### 1.7 Folder Navigation Sync
- **Instant Folder Refresh ([`Main.qml`](file:///home/cody/dev/rust/mailclient/crates/mailapp/qml/Main.qml#L81))**:
  - `selectFolder(path)` calls `backend.sync_folder_now(path)` upon folder selection, ensuring the message list and unread counts refresh immediately upon opening.

### 1.8 Offline Mock Test Suite
- **In-Memory Mock Server ([`MockImapServer`](file:///home/cody/dev/rust/mailclient/crates/mailcore/src/sync/imap.rs#L2246))**:
  - Built using `tokio::net::TcpListener` on ephemeral localhost ports (`127.0.0.1:0`).
  - Simulates raw IMAP wire-protocol exchanges and records client command history.
  - Added mock tests covering:
    1. Capability detection & extension suppression on standard servers.
    2. Fallback on unsupported `SELECT (CONDSTORE)`.
    3. Fallback on failed `UID MOVE`.
    4. Fallback on rejected `CHANGEDSINCE` modifier.
    5. Marking unread messages as `\Seen` before trashing.
    6. Unconditionally storing `\Seen` when trashing even if locally read.
    7. Batch trashing setting `\Seen`.
    8. `move_to_folder` to Trash setting `\Seen`.
    9. Feed tests asserting Trash folders and messages report `unread: 0` / `unread: false`.
  - All unit tests run completely offline in under 0.5s without any live IMAP servers.

---

## 2. What Was Left Out (And Why)

### 2.1 IMAP IDLE (RFC 2177)
- **Status**: Deferred to Milestone 5b.
- **Rationale**:
  - IDLE requires keeping an open, dedicated TCP connection per monitored folder, handling periodic renegotiation every 29 minutes, and gracefully exiting IDLE mode whenever any client command (fetch, flag change, move) needs to be executed.
  - The current model (periodic background polling via `check_interval_secs` + immediate navigation sync on folder click + manual sync) provides a responsive experience without the complexity of managing connection lifecycles and concurrency hazards across multiple folders.

### 2.2 Background System Daemon Outbox Retry Loop
- **Status**: Database schema ready; background service deferred.
- **Rationale**:
  - The SQLite table `send_queue` and retry tracking logic are fully implemented in `mailcore::store::queue`.
  - Currently, unsent messages are retried while the desktop client is running. An external headless daemon that runs continuously when the desktop UI is closed was deferred to keep deployment simple and avoid managing background service units on user systems.

### 2.3 UIDPLUS Extension Tracking (RFC 4315)
- **Status**: Left out.
- **Rationale**:
  - `UIDPLUS` provides `APPENDUID` and `COPYUID` response codes indicating the new UIDs assigned in destination folders.
  - Rather than relying on optional `UIDPLUS` support (which many legacy servers do not implement), the client relies on the next folder sync or refresh to discover the newly placed message. This keeps the sync logic uniform and server-agnostic.

### 2.4 Streaming Partial MIME Parts (RFC 3516 / BINARY)
- **Status**: Left out.
- **Rationale**:
  - Downloading individual MIME body parts via `FETCH ... BODY[1.2]` requires multiple roundtrips per message.
  - For normal email workloads, fetching the full RFC 822 body in one roundtrip is faster and simpler. Attachments are capped at 25MB per file to prevent database bloat, and attachment bytes are only extracted/stored on explicit demand.

### 2.5 Optimistic Concurrency with `STORE ... UNCHANGEDSINCE`
- **Status**: Left out.
- **Rationale**:
  - RFC 4551 allows conditional flag stores using `UNCHANGEDSINCE <modseq>` to avoid overwriting concurrent flag changes from other clients.
  - In a single-user desktop client, user actions (marking a message as read or starred) are intended to be authoritative. If another client changed flags concurrently, the newest state is reconciled on the next flag fetch. Conditional stores would add retry and conflict-resolution overhead with minimal real-world benefit.
