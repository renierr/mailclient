# Codebase Review — Findings Backlog

Read-only review of the whole repo (mailcore, mailapp/Qt, mailffi, native Android), 2026-10-08.
Work top to bottom; each item is self-contained so they can be picked out of order.

- **Severity:** `critical` (crash / data loss / secret exposure), `high` (wrong results, hangs, timeouts),
  `medium` (latent or narrow), `low` (polish / hygiene).
- **Status:** all items pending unless marked. `[verify]` = reproduced or confirmed by reading the code;
  no marker = read from source but not executed. Nothing here was changed in the repo — this file is
  the only added file.
- **AGENTS.md §7** still governs: a Rust fix means `cargo fmt --check` + `cargo clippy -p mailcore -- -D warnings`
  + `cargo test -p mailcore`; QML means `scripts/qml-check.sh`; Android means `./build.sh --android`.

## Suggested fix order

| # | Item | Why first |
|---|------|-----------|
| 1 | **C1** `badge.rs:76` byte-index panic | one malicious sender crashes folder listing on every frontend |
| 2 | **E1/E2** Android backup rules | plaintext passwords + whole mailbox leave the device in cloud backup |
| 3 | **A1** unparseable `schema_meta` version bricks `Db::open` | unrecoverable startup brick |
| 4 | **A4** unbounded `uid in (?,…)` | bulk actions on >32 764 UIDs permanently fail |
| 5 | **B1** SMTP has no socket timeout | one dead SMTP host hangs the whole net thread forever |
| 6 | **C2** quadratic entity decode | ~9 s CPU per crafted mail, blocks every job |
| 7 | **A2** deferred-tx upgrade race | intermittent "database is locked" on attachment save |
| 8 | **D5/D6** GUI-thread DB + file IO | whole-cache JSON + keyring D-Bus on the GUI thread |
| 9 | **C3** transparent full-body link overlay | in-page click theft |
| 10 | **D10** two `ScrollView` width bugs | wrapping/content clipped, long status text unreadable |

---

## A. `mailcore` persistence layer — `db/`, `store/`, `models.rs`

### A1 · critical · version stamp collapse bricks open forever `[verify]`
`db/migrations.rs:144-151`

```rust
let current: u32 = conn.query_row(…).unwrap_or(0);   // inner: parse::<u32>().unwrap_or(0)
if current == 0 {
    conn.execute_batch(SCHEMA_FULL)?;
    conn.execute("insert into schema_meta (key, value) values ('version', ?1)", …)?;  // PK clash
}
```
Any *existing* row whose `value` fails to parse (`''`, `'0'`, `' 23'`, `'v23'`, `'-1'`, `'999999999999'`)
collapses to `0`, so an existing DB takes the fresh-install path and the plain `INSERT` hits the primary
key. One truncated page and the app never starts again; every later open repeats it. A *missing* row is
handled correctly.
**Fix:** `insert … on conflict (key) do update set value = excluded.value`.

### A2 · high · deferred transaction reads before it writes → `SQLITE_BUSY` `[verify]`
`store/messages/attachments.rs:50-51`

```rust
let tx = db.conn().unchecked_transaction()?;
let mut existing = list_attachments(db, message_id)?;   // read snapshot taken here
… tx.execute("update attachments set data = …")
```
`unchecked_transaction()` is a deferred `BEGIN`; the read already holds a snapshot, so if any other
connection commits in between (GUI thread + net thread in `mailapp`; the FRB pool in `mailffi`; the
`--sync-once` CLI that `queue.rs:20` documents as sharing the file) the write upgrade fails immediately —
`busy_timeout` does not cover snapshot-upgrade. Verified: two connections, deferred tx + prior read →
`database is locked`; a write-only deferred tx succeeds.
**Fix:** `TransactionBehavior::Immediate`, or read `existing` before `BEGIN`.

### A3 · medium · failed migration steps still stamp success `[verify]`
`db/migrations.rs:303-310` (+ `:190-198`, `:225-228`, `:246-249`, `:259-262`, `:293-296`)

```rust
if let Err(e) = crate::store::contacts::backfill_sent_counts_from_connection(conn) {
    log::warn!("migration v21: sent-count backfill failed: {e}");
}
… conn.execute("update schema_meta set value = ?1 where key = 'version'", …)?;
```
v21's backfill (`store/contacts.rs:149`) issues one `update contacts set sent_count = sent_count + ?1`
per address with **no transaction**, so a mid-run failure leaves partial credit that is then recorded as
done — never retried. Same shape for v8/v12/v14/v16/v20.
**Fix:** wrap each best-effort step in its own transaction; do not advance `current` past a step that errored.

### A4 · high · unbounded `uid in (?,?,…)` fails at 32 764 UIDs `[verify]`
`store/messages/flags.rs:127-128`

```rust
let placeholders = vec!["?"; clean.len()].join(",");
let sql = format!("{sql_head} uid in ({placeholders})");
```
`db error: too many SQL variables` at the 32 764th UID (3 leading params against SQLite's 32 766).
Reachable via `bulk.rs:44-56` (`bulk::set_read`/`set_starred`, folder-wide selection) and
`sync/imap/engine/mutate.rs:108,124` (`move_uids_to`, `purge_uids`) which receive an **unchunked** group
from `push_due_moves`. Symptom: one bulk delete of >32 k mail permanently fails locally — it burns all
`MAX_PENDING_ATTEMPTS`, the rows are dropped, and every message re-appears.
`sync/imap/engine/sync.rs:136` chunks correctly with `FETCH_CHUNK`; these paths do not.
**Fix:** chunk `uids` at ≤900 per statement inside `execute_over_uids`.

### A5 · medium · migration repair: no transaction, `prepare` inside the loop
`db/migrations.rs:382-408` — `migrate_cid_inline_attachments` re-prepares the statement per message
(N+1) and writes unbatched with no transaction, so a crash leaves a half-repaired attachment set that
(per A3) is never re-run.
**Fix:** prepare once outside the loop; run in one `unchecked_transaction()`.

### A6 · medium · every error becomes "not found"
`store/queue.rs:120-128`

```rust
.query_row(&format!("select {COLS} from send_queue where id = ?1"), [id], row_to_queued)
.map_err(|_| StoreError::NotFound(format!("queue entry {id}")))
```
A locked DB, a corrupt row or an I/O error is misreported as `NotFound`, so callers keep retrying a
permanently broken row. Every other store uses `.optional()?`.
**Fix:** `.optional()?.ok_or_else(|| StoreError::NotFound(…))`.

### A7 · medium · `attachment_has_data` leaks a raw no-rows error `[verify]`
`store/messages/attachments.rs:173-182` — `attachment_has_data(db, 4242)` returns
`database error: Query returned no rows`; `crates/mailffi/src/api/attachments.rs:27` and
`crates/mailcore/src/compose/forward.rs:105` surface that string to the user where "attachment not found"
was meant.
**Fix:** `.optional()?.map_or(Ok(false), |n| Ok(n != 0))`.

### A8 · medium · `unwrap_or_default()` hides column corruption `[verify]`
`store/messages.rs:47-49,60` and `store/queue.rs:75`

```rust
to_addrs: json_vec(&to).unwrap_or_default(),
```
Writing `to_addrs = 'not json'` then reading back yields `to_addrs=[]` with no log line. Worse in
`queue.rs` (`envelope_to: json_vec(&to_raw).unwrap_or_default()`): a corrupt envelope row is presented as
having zero recipients and is claimed/submitted as such. Same at `contacts.rs:543`
(`serde_json::to_value(c).unwrap_or_default()` silently drops a contact from `contacts_json`) and `choices.rs:81`.
**Fix:** `warn!` on parse failure; treat an unparseable `envelope_to` as a hard error.

### A9 · low · "transactional" deletes that are not
`store/contacts.rs:449-463` — `delete_many` is documented/aliased as transactional but issues one
implicit transaction per address; a mid-list failure leaves a partial delete. Same in
`pending_moves::record_failure`/`remove` (`:159-178`) and `bulk::set_read` (`bulk.rs:44-48`).
**Fix:** wrap in `unchecked_transaction()` or collapse to `where address in (?,…)`.

### A10 · low · multi-statement mutations outside the tx helper
`store/settings.rs:266-268` (`set_pending_open` calls `set` twice), `:394-395` (`set_sort` twice), while
`set_many` (`:175-202`) does it in one transaction. A crash between leaves a jump request with an account
but no folder.
**Fix:** route these through `set_many`.

### A11 · low · a *newer* version stamp is silently rewound `[verify]`
`db/migrations.rs:327-332` — a DB stamped `99` opens "successfully" and is rewritten to the current version.
The newer build later re-applies its own migrations over this build's schema.
**Fix:** log loudly (or refuse) when `current > SCHEMA_VERSION`.

### A12 · medium · `save_edit` has no keyring compensation
`store/account_form.rs:404-409` — `secrets.save(...)` lands **before** `accounts::update_connection(...)`.
`create_new` (`:429-434`) deletes the orphaned vault entry on failure; `save_edit` does not, so a failed row
update leaves *new* passwords next to *old* host/user — an account that cannot connect, with no way for the
user to tell which half is stale.
**Fix:** delete the vault entry on `update_connection` failure (or update the row first).

### A13 · low · dead `async fn` holding `&Db` across `.await`
`store/account_form.rs:240-245` — the future is `!Send` (`&Db` is `!Send`). Both adapters deliberately avoid it
(`crates/mailffi/src/api/accounts.rs:75-96` documents "split-phase so the future stays `Send`"), leaving this
wrapper with only its own tests as callers and the FRB pool unavailable to it.
**Fix:** delete it, or drop the `db` parameter.

### A14 · low · silent truncation casts
`store/accounts.rs:17,20`, `store/folders.rs:18-21`, `store/messages.rs:42,61`, `store/queue.rs:72` —
`row.get::<_, i64>(5)? as u16` / `as u32` / `as u64` on every port / uid / count. A port stored as 70000
reads back as 4464; a negative `size` reads back as 1.8e19. Only reachable via a hand-edited DB, but silent.
**Fix:** `u16::try_from(v).unwrap_or_default()` or a checked conversion with a warning.

### A15 · medium · full-table scan + full Rust sort on every keystroke
`store/contacts.rs:399-413` — `suggest(db, prefix, 10)` reads *every* contact and fuzzy-scores in memory;
`idx_contacts_seen` is only used by the empty-query branch (`:382`). `cleanup_candidates` (`:472-487`) is the
same. At a few thousand contacts this is a visible per-keystroke stall in the composer.
**Fix:** prefilter with `like` in SQL, or memoize the contact list between keystrokes.

### A16 · low · v12's folder selector can rename an already-correct path *(suspected)*
`db/migrations.rs:470-476` — `where path like '%&%-%'` then `decode_modified_utf7`; since `sync/imap/utf7.rs:31-33`
maps `&-` → `&`, an already-decoded mailbox literally named `Foo&-Bar` decodes to `Foo&Bar ≠ path` and the
"leftover UTF-7 detection" renames a correct folder to a wrong one. Pre-v12 DBs, one-shot.
**Fix:** guard with `encode_modified_utf7(&decoded) == path` before renaming.

### A17 · low · duplication and dead code
- `store/contacts.rs:380-396`, `:399-413`, `:472-487` — three copies of the same query + row mapping.
- `store/settings.rs:314-329` vs `:487-502` — `get_delay_secs` / `get_sync_interval` are one function twice.
- `store/messages/attachments.rs:212-218` `delete_attachments_for_message` — **verified** no non-test callers.
  Dead per AGENTS.md; delete it and its test.
- `store/undo.rs:171-176` — `messages::get_by_uid` in a loop over a whole selection: N+1 over `get_by_uid`.

### A18 · low · column-order drift between `schema.sql` and an upgraded DB `[verify]`
`ALTER TABLE ADD COLUMN` always appends, so an upgraded DB puts `accounts.from_name`, `folders.server_total`,
`messages.from_name`, `attachments.data`, `contacts.alias`, `send_queue.raw_mime` at the end, not at their
`schema.sql` position. Verified no `select *` anywhere and every `row_to_*` names columns explicitly, so it is
harmless today — but `schema.sql` is not a faithful description of an upgraded DB.
**Fix:** record the caveat in the `migrations.rs` header, or rebuild the affected tables once to restore canonical order.

---

## B. `mailcore` sync & networking — `sync/`

### B1 · high · SMTP has no socket timeout, and blocks the async runtime `[verify]`
`sync/sender/client.rs:79-81,96-109,428` and `sync/sender/dsn.rs:40-46`

```rust
fn transport(&self, password: &str) -> Result<SmtpTransport> { self.transport_with_timeout(password, None) }
… let response = self.transport(password)?.send_raw(&envelope, raw)?;          // client.rs:428
let mut conn = SmtpConnection::connect((host, port), None, &hello, wrapper, None)?;  // dsn.rs:40
```
lettre 0.11 with `smtp-transport`/`pool` is the **blocking** transport; `send_raw` does connect+TLS+AUTH+DATA
inline. `.timeout(None)` is not "the default" — it disables both the connect timeout and the read/write
timeouts, so the doc comment "sends keep the transport default" is wrong. The `mailclient-net`
current-thread runtime blocks forever on a half-open SMTP socket and every queued IMAP job stops until
restart. The DSN path has no timeout at all.
**Fix:** `.timeout(Some(secs(30)))` everywhere, and move the blocking submit off the runtime (`spawn_blocking`
or a dedicated SMTP thread).

### B2 · medium · `COMMAND_TIMEOUT` bounds each *read*, not the command
`sync/imap/session.rs:122-128` (and `session/idle.rs:150-155`, `read_greeting`)

```rust
loop { let event = tokio::time::timeout(COMMAND_TIMEOUT, self.stream.next(&mut self.client)) …
```
Each iteration gets a fresh 30 s. A server that sends any untagged noise at <30 s intervals (`* OK Still
here`, a quota notice, or a hostile drip) keeps the loop alive indefinitely while the tagged reply never arrives.
**Fix:** `let deadline = Instant::now() + COMMAND_TIMEOUT` once before the loop.

### B3 · medium · no error classification; a dead session is reused
`sync/imap/session.rs:151-156,163-171`, `session/mailbox.rs:63-77,162-181`, `sync/headless.rs:285`
Timeouts, `BYE`, stream errors and a mere `NO`/`BAD` all come back as `StoreError::Network`. (a) After a timeout
the abandoned command's late reply is delivered into the *next* command's `collected_data`
(`session.rs:139-141`), so `uid_search`'s `Data::Search` (`session/mailbox.rs:284-288`) can mix in stale hits
and feed the expunge diff. (b) `select()`'s and `uid_fetch_flags_changesince()`'s fallbacks fire on *any*
error — including a dead socket — re-issuing a command on a broken stream and clearing
`condstore`/`qresync` for the session because of a connection blip. `sync_account` swallows the error and
continues with the same session on the next folder.
**Fix:** distinguish transport-fatal from server-`NO`, mark the session dead on the former, stop the sweep to
force a reconnect.

### B4 · medium · unbounded accumulation of one command's response
`sync/imap/session.rs:139-141` + `sync/imap/types.rs:18,44` — `uid_fetch_messages` asks for `FETCH_CHUNK = 100`
full `BODY.PEEK[]` bodies in one command with no byte budget. The 25 MiB cap (`MAX_ATTACHMENT_BYTES`) applies
*after* parsing, i.e. after the whole response is resident. One 100-message chunk of newsletters is gigabytes
in RAM; a hostile or merely enthusiastic server OOMs the process.
**Fix:** budget per command (abort past e.g. 64 MiB) and/or smaller chunks plus `BODY.PEEK[]<0.N>` partials.

### B5 · low · unbounded net-job queue; one pool thread per send
`mailapp/src/bridge/worker.rs:143-172` (`mpsc::channel()` is unbounded; `spawn_flag_push`'s
`let _ = net_tx().send(..)` never coalesces) and `sync/sender/client.rs:403-434` — every `submit_raw` builds a
throwaway `SmtpTransport`, spawning a `lettre-connection-pool` thread that lives up to its 60 s `idle_timeout`.
A 50-row outbox flush spawns 50 threads.
**Fix:** build the transport once per flush; cap/coalesce the net queue.

### B6 · low · one sleeping OS thread per undo
`mailapp/src/bridge/worker.rs:135-141`, `crates/mailffi/src/net.rs:280-287` — `std::thread::spawn` + `sleep`
per undoable action. Unbounded if the user hammers delete/move.
**Fix:** coalesce on the existing in-flight table (see E10).

### B7 · medium · UIDVALIDITY change wipes the cache before the replacement exists
`sync/imap/engine/sync.rs:88-95`

```rust
if validity_changed { messages::delete_by_folder(db, folder_id)?; }
```
Deletion happens at the top of `sync_folder_window`; every later step (flag fetch, backfill, Sent copy,
connection drop, task abort in `push.rs:181-187`) can leave the folder locally **empty** with the
badge/notification baseline lost until the next successful sync.
**Fix:** fetch the new window first (staging table) or defer the delete / write a "resync pending" marker.

### B8 · low-medium · step 3 uses the stale modseq after a validity change
`sync/imap/engine/sync.rs:222-230` vs `:191-195` — step 2 correctly zeroes `flags_since` when
`validity_changed`, but step 3 (`older_existing`, messages outside the window) uses `folder.highest_modseq`,
a modseq from a mailbox incarnation that no longer exists.
**Fix:** `let flags_since = if condstore_enabled && !validity_changed { folder.highest_modseq } else { 0 }` in step 3 too.

### B9 · low · Trash `\Seen` sweep clobbers starred/draft locally
`sync/imap/engine/sync.rs:55-59` with `store/messages/flags.rs:89-93`

```rust
let _ = messages::set_flags_by_uid(db, account_id, folder_id, *uid, true, false, false);
```
`set_flags_by_uid` writes **all three** columns, so a starred clean row in Trash loses its star and
`is_draft` is forced false until the next sync re-fetches.
**Fix:** a read-only setter, or pass the row's current starred/draft.

### B10 · low-medium · Trash sweep writes local state the server may not have
`sync/imap/engine/sync.rs:49-60` — the STORE failure is only `log::warn`ed, yet local rows are marked read
unconditionally. A server rejecting the STORE (read-only mailbox, quota) leaves a permanent local/server
disagreement that nothing re-pushes, because the rows are not `flags_dirty`.
**Fix:** update locally only after a successful STORE, or mark the rows dirty.

### B11 · low · partial move deletes rows for UIDs the server may still hold *(suspected)*
`sync/imap/engine/mutate.rs:107-109` — `uid_move` (COPY+STORE+EXPUNGE) can partially apply (per-UID
ACL/quota) before failing; `delete_many_by_uids` then removes rows for messages still in the source folder.
Self-healing only for UIDs inside the next window's search range.
**Fix:** delete only the UIDs the server confirmed, or re-SEARCH the source before deleting.

### B12 · low · `move_uids_to` pre-marks `\Seen`, leaving both sides inconsistent on failure
`sync/imap/engine/mutate.rs:99-106` — a failed `uid_move` after a successful `\Seen` STORE leaves
local-unread / server-read with no dirty flag to reconcile it.
**Fix:** mark seen after the move succeeds.

### B13 · low · sessions checked back into the pool after an error
`sync/pool.rs:89-92,113` vs `sync/attachments.rs:30-33` — the pool documents that anything but clean
completion should drop the session, but `imap.checkin()` also runs on the error path. It self-heals via
`is_healthy()`, but a timed-out session goes back with stale reply data pending.
**Fix:** `drop(imap)` / explicit discard on the error path.

### B14 · low · `disconnect()` (a syscall) runs under the global pool mutex
`sync/pool.rs:103-117,133-143` — harmless today, but it serializes every account's checkout behind a close.
**Fix:** take the session out of the guard, drop the lock, then disconnect.

### B15 · medium · `task.abort()` can stop a push account mid-`push_check`
`sync/push.rs:181-192`

```rust
tasks.retain(|id, task| { let keep = …; if !keep { task.abort(); } keep });
```
Abort lands at an await point inside `push_check` → `sync_account`, which may be between
`sync_folder_window`'s `delete_by_folder` and the refetch (see B7) or between `sync_accounts` and
`mark_checked`. Nothing is surfaced; the lock / `ImapSync` drop is the only cleanup.
**Fix:** abort only when idle, or keep a cancellation flag the task honours at a safe point.

### B16 · low · shutdown waits for tasks with no bound
`sync/push.rs:196-199` — `for (_, task) in tasks { let _ = task.await; }`. Tasks are asked to stop, never
aborted. A task stuck in the no-timeout SMTP send (B1) or a 30 s-per-read IMAP command (B2) keeps the push
thread alive for minutes.
**Fix:** `abort()` then `await` with a timeout.

### B17 · low · push swallows real errors, keeping the account "healthy"
`sync/push.rs:373-382` — `report.errors` are ignored unless the session fails a NOOP, so a permanently
failing outbox/SMTP never increments `failures` and never enters the 30 s→15 min backoff.
**Fix:** surface send-path errors into the witness for a failing account.

### B18 · low-medium · sync lock is TOCTOU-racy and recursively unbounded
`sync/headless.rs:498-527` — an empty or partially-written pid file parses to nothing → `is_none_or(...)` yields
"stale" → `remove_file` → retry. Two processes can each conclude the lock is stale and each hold it;
`acquire_sync_lock` also recurses without a depth bound (stack overflow if the path keeps coming back
"stale", e.g. a directory at that path).
**Fix:** `flock` / atomic create-and-verify, and a bounded loop instead of recursion.

### B19 · medium · per-message attachment budget is only part-count × 25 MiB
`sync/imap/parse.rs:150,157-159` + `sync/imap/engine/sync.rs:439-470` — `fetch_attachments` parses with
`with_bytes = true` and stores up to `MAX_ATTACHMENTS_PER_MESSAGE` (50) parts × `MAX_ATTACHMENT_BYTES`
(25 MiB) = 1.25 GiB into SQLite for a single message. Only *inline* images have a per-message budget
(`MAX_INLINE_BYTES_PER_MESSAGE`).
**Fix:** a per-message byte budget for the download path (see C4).

### B20 · low · systemic blocking SQLite on the async runtime
Every `messages::upsert` / `set_flags_by_uid` inside `sync_folder_window`'s loops
(`engine/sync.rs:201-218,263-288,304-309`) is a synchronous rusqlite commit on `mailclient-net`; step 5
deletes one row per statement. Acceptable today, but a 200-message window plus a large expunge sweep is a
multi-second stall of that thread, and it compounds with B1/B2.

**Verified clean (no action):** certificate verification is on for both IMAP (`tls.rs:11-33`, rustls +
native + webpki roots, `ServerName` from the host, STARTTLS upgrade before `login`) and SMTP
(`TlsParameters::new` rejects invalid certs/hostnames; `Tls::Required`/`Wrapper` only, no
opportunistic/downgrade path; plaintext requires an explicit `security = "none"`). No `SendPolicy` bypass —
every send funnels through `enqueue_send`'s `policy.check(&rcpt_refs)` (`client.rs:183`, covering
To+Cc+Bcc), and the harness's `from_env()` is gated behind `-- --live` + `MAILCLIENT_SEND_TEST_MAIL=1`.
No `std::sync::Mutex` guard is held across an `await` anywhere in `sync/`. No credential/password/body
logging. No panic reachable from a hostile server in `parse.rs` / `utf7.rs` / `seq.rs` /
`discovered.rs` — verified specifically for each slicing site.

---

## C. `mailcore` untrusted input — `html/`, `mime.rs`, `calendar`, `vcard`, `feed`, `search`

Treat all bodies, headers, attachment names and vCard/iCalendar data as attacker-controlled.

### C1 · critical · `badge.rs:76` byte-index panic on a non-ASCII sender `[verify]`
`crates/mailcore/src/badge.rs:76`

```rust
let punycode = label.len() >= 4 && label[..4].eq_ignore_ascii_case("xn--");
```
`label.len() >= 4` does not guarantee that byte 4 is a char boundary. `domain_label`
(`badge.rs:93-110`) pops the TLD first, so a **single-label** domain with a multi-byte char is enough —
no dot needed. Verified with the real `domain_label`:

| sender | label | `len()` | `label[..4]` |
|---|---|---|---|
| `a@abcö.com` | `abcö` | 5 | **panics** (byte 4 inside `ö`) |
| `a@x🎉.de`  | `x🎉` | 5 | **panics** |
| `a@abc🎉`    | `abc🎉` | 7 | **panics** |
| `a@ex.co.uk` | `co` | 2 | fine |

Confirmed panic message: `byte index 4 is not a char boundary; it is inside 'ö'`.
Reachable from `feed::messages_list_json_paged` (feed.rs:478), `feed::hit_json` (feed.rs:907),
`feed::message_json` (feed.rs:572) and `feed::contact_json` (feed.rs:662) — listing a folder, showing a
search hit, or opening a message. Behind the cxx-qt/JNI boundary the unwind aborts the process, not just
the job.
**Fix:** `let punycode = label.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("xn--"));`

### C2 · high · quadratic entity decode: ~9.3 s CPU on one crafted mail `[verify]`
`html/entities.rs:14`

```rust
if let Some(semi) = s[i..].find(';').filter(|n| *n < 24) {
```
`find(';')` scans to the **end of the string** before `< 24` is applied. Text nodes are split at `<`, so a
512 KB tag-free body is one node, and `sanitize` feeds it to `decode_entities` (`html/sanitize.rs:147`) and
`html_to_text` does it again. Measured with the exact algorithm:

| n | elapsed |
|---|---|
| 20 000 | 12.8 ms |
| 40 000 | 50.5 ms |
| 80 000 | 210 ms |
| 512 000 | **9.32 s** |

Input: a body of `"&".repeat(512_000)`. ~19 s of the net/GUI thread per crafted mail; a mailing list of these
starves every other job.
**Fix:** bound the window before searching — `s.as_bytes()[i+1..].iter().take(24).position(|&b| b == b';')`
— and treat "no `;` within 24 bytes" as a literal `&` without scanning further.

### C3 · medium · CSS clickjacking: invisible full-body link overlay survives sanitizing
`html/css.rs:70-73` allows `display`, `width`/`height`, `margin`, `opacity`; `allowed_display` permits
`block`; `safe_value("0")` passes. `presentational(tag, "style", v)` (`sanitize.rs:81`) applies it to **any**
allowed tag, including `a`.

```html
<a href="https://evil.example.net/" style="display:block;width:100%;height:1400px;opacity:0;margin:-16px 0">…</a>
```
The reader document CSS (`html/reader.rs:293-307`) only sets `a{color}` — no `pointer-events` guard — so the
transparent rectangle covers the mail and steals the click. The CSP keeps `style-src 'unsafe-inline'`, so
nothing blocks it downstream.
**Fix:** clamp `opacity` to a visible minimum (or drop the declaration below ~0.15), and refuse
`display:block` + size on `a`.

### C4 · medium · no parse-side size cap: a 25 MB `.ics`/`.vcf`/DSN is parsed on every open
The caps only gate what sync *pre-caches* (`sync/imap/parse.rs:239-253`, 64 KB / 256 KB). On an explicit
download `fetch_attachments` runs `extract_attachments(&parsed, true, …)` and `replace_attachments` stores
**every part up to 25 MiB**; the feed then parses those bytes with no cap:

```rust
// feed.rs:592-598 (also 629 parse_vcard_bytes, report.rs:391 parse_dsn)
if let Some(full) = messages::get_attachment(db, att.id) {
    if let Some(bytes) = full.data.as_deref() {
        if let Some(mut event) = crate::calendar::parse_ics_bytes(bytes) { … }
```
`parse_ics` starts with `unfold(ics_data)` (`calendar.rs:153`) — a full 25 MB copy — and `stack.push(comp)`
once per `BEGIN:` line (`calendar.rs:192`), so an `.ics` with 1 M `BEGIN:X` lines builds 1 M `String`s,
repeated on every message selection.
**Fix:** re-apply the caps in the feed (`<= 64 * 1024` before `parse_ics_bytes`, `<= 256 * 1024` before
`parse_vcard_bytes`/`parse_dsn`) and cap `calendar`'s component stack (C15).

### C5 · medium · unbounded DSN recipient expansion
`report.rs:152` — `parse_dsn` has no recipient cap (unlike `vcard.rs:15`'s `MAX_ENTRIES`). Every block with a
`Final-Recipient` becomes a 7-field struct serialized into `message_json`. A 20 MB `message/delivery-status`
part of ~600 000 `Final-Recipient:` blocks yields several hundred MB of `ReportRecipient`s plus a multi-MB
JSON payload, on every message open.
**Fix:** cap `dsn.recipients` (e.g. 50) as the vCard parser does.

### C6 · medium · an unclosed drop-content tag swallows the rest of the message
`html/sanitize.rs:44-58` — nothing but a matching close tag ever lowers `drop_depth`; not `</html>`, not
`</body>`, not EOF. `drop_content_tag` includes `style`, `head`, `template`, `form`, `title`, `noscript`
(`html/tags.rs:100-117`). Input `<p>visible</p><template>` (or `…<style>p{color:red}`, or any mail whose
`</style>` was mangled in transit) silently discards everything after it — `if drop_depth == 0` at
`sanitize.rs:146` gates every text run. Content-loss DoS: a mail can hide its own body from the reader, or
hide a tracked signature block from a spam filter that consumes this text.
**Fix:** reset `drop_depth` to 0 at EOF, and/or treat `</html>`/`</body>` and a second document-level
`<style>` as closing.

### C7 · low-medium · `is_public_remote` misses link-local / ULA / CGNAT / non-dotted hosts
`html/urls.rs:42-63` rejects `localhost`, `127.`, `10.`, `192.168.`, `::1`. Missing `169.254.0.0/16`
(incl. cloud metadata `169.254.169.254`), `100.64.0.0/10`, `fc00::/7`, `fe80::/10`, `0.0.0.0`, and
`inet_aton` forms (`2130706433`, `127.1`). An `<img src="http://169.254.169.254/latest/meta-data/…">` is
fetched whenever the user enables remote images — the response is not readable, so this is internal port
scanning / metadata probing, not exfiltration.
**Fix:** parse the host as an IP and reject all non-global ranges (`!ip.is_global()`), keeping the literal
denylist as a fallback.

### C8 · low · a bogus comment/PI with no closing token swallows the rest
`html/tags.rs:132-139` — for `<!-->` the search starts *past* the closing `>`, finds nothing, and consumes
the rest of the document; `<!--->` likewise. Input `<!--><p>everything after this is gone</p>`.
**Fix:** if `bytes[i+3] == b'>'` close the comment at `i+4`; likewise for `<!--->`.

### C9 · low · a PI with no `?>` swallows the rest
`html/tags.rs:145-150` — returns `(None, bytes.len())` when `find_sub(bytes, b"?>", i)` finds nothing. HTML
ends a bogus comment at the first `>`. Input `<p>ok</p><?x ><p>rest</p>` → `rest` never appears.
**Fix:** end at the first `>` as HTML does, and only treat `<?xml … ?>` as a PI.

### C10 · low · inline-image expansion escapes `MAX_OUT_BYTES`
`html/inline.rs:120-160` — `MAX_INLINE_BYTES_PER_MESSAGE` is 6 MB of raw bytes, and each `cid:` hit appends
`base64_encode(&img.data)` (~4/3 → ~8 MB) directly to `out` with no `MAX_OUT_BYTES` re-check (unlike
`sanitize`'s `push_capped`). Four 1.5 MB inline PNGs referenced from `<img src="cid:…">` produce an ~8 MB
document that `reader::document` embeds.
**Fix:** count the produced length against `MAX_OUT_BYTES` (or a document budget) and stop substituting.

### C11 · low · `is_body_referenced` re-lowercases the whole body once per attachment
`feed.rs:691-695` — `is_body_referenced` → `img_cid_references(html)` → `html.to_ascii_lowercase()`
(`html/inline.rs:66`), a full copy + scan of the body **per attachment** (up to 50). 512 KB × 50 ≈ 25 MB of
copy+scan per message open.
**Fix:** compute `let refs = html::img_cid_references(body_html)` once outside the filter and test `refs.contains(...)`.

### C12 · low · unbounded Tier-2 similarity scan, no SQL `LIMIT`
`similar.rs:263-275` — `let mut rows = stmt.query(params![account_id, target_from])?;` with no limit, and
every row's subject normalized in Rust. The loop only breaks once enough *matching* ids are collected, so a
sender with a large history and no matches walks the whole set on the feed thread.
**Fix:** add `limit ?N` (a small multiple of `remaining`) or stream with an explicit cursor.

### C13 · low · dead branch in mime sniffing
`mime.rs:299-313`

```rust
if normalize_declared(declared) != sniffed { return None; }   // line 310
return None;                                                  // line 312
```
Both arms identical. A docx/xlsx whose declared type is a *different specific* type (e.g. `application/msword`
on OOXML bytes) keeps the wrong type, so `paths::safe_attachment_name_for_mime` leaves the wrong extension
and the OS opens the wrong app.
**Fix:** `return Some(sniffed.to_string())` for the mismatch case, or collapse the block.

### C14 · low · fragile fixed-width slice in vcard
`vcard.rs:369` — `s[..1].make_ascii_uppercase();` is safe only because `s` is always one of
`"mobile" | "fax" | "pager"` today. Same class as C1.
**Fix:** `s.get_mut(..1).map(str::make_ascii_uppercase)`.

### C15 · low · calendar component stack has no cap
`calendar.rs:157-211` — `stack.push(comp)` runs per `BEGIN:` and only `END`/`truncate(pos)` ever shrinks it,
and a non-matching `END` silently does nothing. The main amplifier behind C4.
**Fix:** cap `stack.len()` and/or bail out after N components.

**Checked and found sound (no action):**
- Tag/attribute allow-list: `allowed_tag` + `presentational` + the `a`/`img` match allow **no** URL sink other
  than `href`/`src`; `background`, `srcset`, `poster`, `formaction`, `xlink:href`, `ping`, `usemap`, `data`,
  `dynsrc` are all dropped; namespaced tags (`svg:script`) are rejected by the `is_ascii_alphanumeric || '-'`
  name check (`tags.rs:193`); `<template>`/`<script>`/`<style>`/comments/PIs are whole-dropped.
- URL scheme check is an allow-list (`urls.rs:77`), so `java\tscript:`, `&#106;avascript:`, `data:`,
  `vbscript:`, `file:` never survive.
- Single-decode/single-escape ordering — `decode_entities` then `escape_text`/`escape_attr`;
  `&amp;lt;` → `&lt;` → `&amp;lt;`. No double-decode mXSS.
- `reader::document` puts `default-src 'none'; img-src data:; form-action 'none'; base-uri 'none'; frame-src 'none'`
  in, and mail can never reach `<head>` (`head`/`style`/`meta`/`base` are drop-content or not allowed).
  `extra_css.replace('<', "")` keeps a trusted toolkit stylesheet from closing the element.
- Search: `fts_query`/`fts_term` quote every term and strip `"`/`*` via `clean` (`search.rs:230-286`); all
  values are bound `?n` parameters — no string concatenation of user terms into SQL (`feed.rs:837-865`);
  `LIKE` is used nowhere.
- Attachment names (`paths.rs:133-227`): basename-only, control chars and `:*?"<>|` neutralised, NUL → `_`,
  `..`/all-dots → fallback, Windows device names prefixed, ADS-splitting `:` removed, byte-capped on a char
  boundary.
- Outgoing path: `to_group_name` (`addresses.rs:61`) rejects anything non-ASCII/CRLF and falls back to
  `undisclosed-recipients`; lettre's RFC-2047/`quoted_string::encode` routes a CR/LF-bearing Subject or
  display name into base64, so no header injection from composer/quoted text.
- No `log::` call in any reviewed file carries a body, address or path.
- The vCard `url`/`emails`/`phones` and calendar `organizer` are rendered as plain text in both frontends, so
  there is no URI-injection sink; the Android WebView runs with JS off, no JS bridge, and a null base URL.

---

## D. Qt frontend — `crates/mailapp/`

### D1 · high · `expect()` on the GUI thread in the net-thread bootstrap
`crates/mailapp/src/bridge/worker.rs:143-172`

```rust
let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime for mailclient-net");
… .spawn(move || { … }).expect("mailclient-net thread");
```
`net_tx()` is lazily initialised from the first `spawn_job`/`spawn_flag_push`, i.e. from inside a
`#[qml_element]` method on the GUI thread. A runtime-build failure or thread-spawn failure (rlimits) aborts
the GUI process from a QML click, and every later job silently no-ops (the `let _ = net_tx().send(...)`
swallows it).
Otherwise clean: no `unwrap()`/indexing in the `Bridge`/`SettingsBridge` invokables; the
`uid < 0` / `folder_id < 0` / `Ok(db) = shared_db()` guards are applied consistently.
**Fix:** initialise the runtime at startup and return an error to QML instead of `expect`.

### D2 · medium · unchecked narrowing on a destructive bulk path
`crates/mailapp/src/bridge/messages.rs:40`

```rust
let uid = x.as_u64().ok_or_else(|| "invalid selection".to_string())? as u32;
```
`parse_hits_json` guards the same cast (`messages.rs:76-80`: `if folder.is_empty() || uid == 0 || uid > u64::from(u32::MAX)`).
A payload uid above `u32::MAX` wraps and `mark_read_many`/`set_star_many`/`delete_many`/`archive_many`/
`move_many`/`purge_many` (`bridge/bulk.rs:23-99`) act on a *different* message id. Today the feed cannot
produce one — it is a missing guard on a destructive path.
**Fix:** mirror the `parse_hits_json` guard.

### D3 · medium · every message API crosses the bridge as `uid: i32` while `mailcore` UIDs are `u32` *(suspected)*
`bridge.rs:221,226,279,285,291,296,302,312,317,322,415,421,426,433,491,496,506,514,520,525,537,647,653` vs
`mailcore/src/models.rs:134`. A UID ≥ 2³¹ arrives negative and every method bails out
(`messages.rs:104-107`), so such mail cannot be opened, starred, deleted or exported from Qt at all.
`find_similar_json` is worse: `uid as i64` (`messages.rs:221`) sign-extends the negative value into the SQL
`uid = ?` comparison, so it silently matches nothing. Real servers exceed 2³¹ on large mailboxes — hence
"suspected" rather than confirmed.
**Fix:** widen to `i64`/`u32` for uid and add a range guard.

### D4 · low · uid/attachment ids kept in QML `int`
`EmlExportDialog.qml:13`, `MessageView.qml:93`, `Main.qml:61` use 32-bit signed properties/signals.
`MessageView.showRemoteOnce` compares a `double` (`root.message.uid`, MessageView.qml:130) against an `int`
(`messageUid`), which disagree above 2³¹. Attachment ids are the same shape: feed/`i64`
(`messages::get_attachment(db, attachment_id as i64)`, `messages.rs:386`) squeezed through
`attachment_id: i32` (`bridge.rs:415,421`). Low because SQLite rowids that high are implausible.
Verified-correct for contrast: `bridge.rs:877-878` (`.min(i32::MAX as u64) as i32`), `messages.rs:250,116,135`.

### D5 · high · whole-cache JSON re-serialised on the GUI thread per interaction
`crates/mailapp/src/bridge/worker.rs` (net rule verified sound) vs `crates/mailapp/src/bridge.rs:861-910`
`push_feeds` — `shared_db()` (`bridge.rs:768-795`) is a leaked per-thread `rusqlite::Connection` used from
~40 `#[qml_element]` methods. Documented as deliberate, but silent where it hurts:
`push_feeds` re-serialises the **entire folder cache** and assigns it to a `QString` property on the GUI
thread — on every `open_message`, `toggle_star`, `mark_read`, `select_folder`, `set_sort`, and after every
finished job (`worker.rs:223-225`), with the limit set to the full cached count
(`bridge.rs:873`, `feed::messages_list_json_paged(db, folder_id, cached, 0)`).
Also per keystroke: `MessageList.qml:381` calls
`list_filter_keep(JSON.stringify(filter), JSON.stringify(rows))` — the whole feed stringified into Rust and
an index array back — plus `search_json`/`contacts_json` (`bridge.rs:924,176`).
**Fix:** paged/partial feed pushes; debounce keystroke-driven work onto the net thread.

### D6 · high · file IO and OS IPC on the GUI thread
- `bridge/composer.rs:81-86` → `sync::sender::inline`: `std::fs::read(&path)` then base64 — reads up to
  `MAX_INLINE_IMAGE_BYTES` (1.5 MB) and pushes the whole `data:` URL across the bridge, from
  `Composer.qml:416` inside the drop handler.
- `bridge/composer.rs:125,144` → `stage_forward_files`/`stage_resend_files`/`stage_draft_files`
  (`compose/forward.rs:97`, `compose/drafts.rs:191`) write attachment copies to temp dirs **and** run
  `prune_stale_draft_dirs` (`paths.rs:268-293`, a `read_dir` walk plus `remove_dir_all`) — all on the GUI thread.
- `maintenance.rs:41-46` `cleanup_temp` → `remove_dir_all` on the GUI thread; `maintenance.rs:21-29` stats the
  DB file and walks the temp dir on the GUI thread (Settings.qml:1040-1045 admits it).
- `bridge/accounts.rs:56` (`add_account` → `account_form::save` → `auth::save_account_secrets` →
  `keyring::Entry`) and `accounts.rs:119` do Secret Service D-Bus round trips on the GUI thread; a hung
  gnome-keyring/kwallet freezes the window.
**Fix:** move these behind `net_tx()` or a dedicated IO job.

### D7 · medium · a SQLite read inside a property *binding*
`Main.qml:1004-1005` — `autoSyncMinutes: … appSettings.sync_interval_for(backend.current_account_id)` →
`account_settings::sync_interval` (`store/settings.rs:38-42`). Re-evaluated on every account switch and every
`syncSettingsRevision++` (Settings.qml:1866).
**Fix:** cache the value in `SettingsBridge` behind the existing revision counter.

### D8 · medium · a new OS thread per undoable action
`bridge/worker.rs:135-141` `spawn_push_after_grace` spawns a **new OS thread per action**, each sleeping
grace+1 s. N deletes in one session = N sleeping threads, unbounded by any cap. (Same defect on Android: E10.)
**Fix:** coalesce into one sleeping task keyed on the in-flight table.

### D9 · low · timer polling does a DB read
`Main.qml:993-999` `pendingOpenTimer` (2 s repeat) calls `consume_pending_open()` → a SQLite settings read on
the GUI thread. Documented as cheap.

### D10 · high · two `ScrollView`s whose content does not bind width to the ScrollView's own id
`Main.qml:1920-1926` and `Composer.qml:772-781` — the exact pattern the rules forbid ("never
`parent.availableWidth` — ScrollView reparents its children"):

```qml
ScrollView { Layout.fillWidth: true; Layout.fillHeight: true; clip: true
    TextArea { id: statusTextArea; text: root.statusText; wrapMode: TextArea.WrapAnywhere; … } }
```
Both children fall back to text-dependent implicit width, so the status text wraps at the wrong width and the
HTML-source editor wraps far narrower than the pane. Every other ScrollView in the app does it correctly
(Settings.qml:638,686,741,798,897,1049,1145; MessageView.qml:1326; AccountSetup.qml:219).
**Fix:** give each ScrollView an `id` and bind the child's width to it.

### D11 · medium · core protocol parsing in JS
- `Main.qml:1456-1464` `undoMove(batch)` re-implements how undo batches are encoded — `batch.split(",")`,
  while Rust joins them in `bridge/bulk.rs:193-197`.
- `Main.qml:777` `parseInt(nl < 0 ? r : r.slice(0, nl), 10)` decodes the `"<id>\n<folder>"` pending-open
  payload in JS.
- `MessagesView.qml:170-181` `joinFileUrl` rebuilds a percent-encoded `file://` URL Rust already has
  (`bridge/messages/files.rs:8-24`).
- `MessagesView.qml:376-384` `baseName` re-implements filename decoding.

Presentation-only logic is fine in QML; these are shared-protocol rules and belong in `mailcore`.
**Fix:** expose the parsing from Rust and have QML consume the result.

### D12 · medium · every reader resize rebuilds a Chromium page
`MessageView.qml:115` `onFitLayoutChanged: root.reloadHtml()` combined with `fitBelow`/`fitLayout`
(MessageView.qml:79-81) — every resize re-runs `reader_fit_below` over the whole document, rebuilds via
`wrapDoc`, and `loadHtml`s a fresh page, losing scroll position and flickering during a drag.
`reader_document` (`messages.rs:253-283`) also re-reads `headerBlock.height` and re-quotes the theme palette
each time.
**Fix:** only reload when the layout bucket actually changes; keep the fit decision in Rust and expose a
`fit_changed` signal.

### D13 · low · the list mutates a feed it does not own
`MessageList.qml:454-457` — `hits[i].key = hits[i].folder_id + ":" + hits[i].uid;` writes into `Main.qml`'s
`searchRows` objects while rebuilding. `MessageList.qml:832` `onContentYChanged: root.rememberScroll()`
calls `indexAt`/`itemAtIndex` on every scroll-pixel change during a flick.
**Fix:** build a local proxy list; throttle scroll memory.

### D14 · medium · non-resizable dialogs, against the stated rule
Only the large managers use `AppDialog`; every small aux dialog is a plain `Dialog` with a fixed `width:`
(and often a fixed `height:`) and no resize grip: `Main.qml:1724-1730` (`deleteConfirm`),
`Main.qml:1793-1799` (`purgeConfirm`), `Main.qml:1871-1877` (`statusDetailsDialog`, also
`height: Math.min(380, root.height - 64)`), `MessageView.qml:1125-1130` (`examineLinkDialog`),
`MessageView.qml:1288-1294` (`headersDialog`, fixed width **and** height), `Composer.qml:938-944,989-995,1050-1056`,
`Accounts.qml:177-183`, `Settings.qml:1267-1272`, `components/ImagePlacementDialog.qml:10-26`. The
status-details dialog is the practical loss: a long error sentence cannot be enlarged.
**Fix:** migrate to `AppDialog` with geometry memory.

### D15 · low · main window width expression has no floor
`Main.qml:20-21` `width: Math.min(1320, Screen.desktopAvailableWidth - 80)` goes negative on a screen
narrower than 80 logical px (clamped by `minimumWidth: 380`, but the initial geometry is nonsense).
Otherwise the responsiveness rules are followed: wrapping labels carry `wrapMode` + bound width, `RowLayout`
children that must yield carry `Layout.minimumWidth: 0` (BulkActionBar.qml:37, Accounts.qml:98-135,
Folders.qml:156, Outbox.qml:127-169, MessageView.qml:830, Settings.qml:1189, AccountSetup.qml:293),
`Flow`s are `Layout.fillWidth` (Settings.qml:1108,1230; ComposerAttachmentTray.qml:40).

### D16 · medium-high · the reader payload copies each body three times
`mailcore/src/feed.rs:562-565`

```rust
"attachments": files, "body_text": plain, "body_html": body_html,
… "body": legacy_body,     // legacy_body is a clone of whichever of the two was chosen
```
A large HTML mail is serialised 3× into one JSON string, copied into a QML JS object (`Main.qml:198`), held in
`currentMessage` while `MessageView` also builds a wrapped document — and `Main.reloadMessages()` re-fetches
the whole payload after every job finish, star toggle, bulk action and sort change. On top of that,
`MessageView.showRemoteOnce` (MessageView.qml:130) pulls the same body again through the separate
`message_html` route (bridge.rs:216-220 documents why).
**Fix:** drop `legacy_body`, paged the feed, and cache the reader document per message.

### D17 · medium · `messages_json` always carries the full local cache
`bridge.rs:873` — a folder at `MAX_MESSAGE_LIMIT` (2000) pushes 2000 rows × ~15 fields as one `QString`
property on every feed rebuild (every star toggle, every message open, every job completion), with the
previous string still alive until Qt swaps the property.
**Fix:** page the feed or diff it.

### D18 · low · one `WebEngineView` kept alive for the app's lifetime
`Composer.qml` is a `Dialog` parented to `Overlay.overlay` holding `EditorFrame`'s `WebEngineView`
(EditorFrame.qml:118); closing the dialog hides it but does not release the page, and its 200 ms
`document.queryCommandState` poll (EditorFrame.qml:158-163) keeps running whenever the dialog is
invisible-but-`ready`.
**Fix:** destroy the WebEngineView on close (or stop the poll while hidden).

### D19 · low · a deliberately leaked connection per bridge thread
`bridge.rs:791` `Box::leak`s a `rusqlite::Connection` per thread that touches the bridge (documented, bounded
at GUI + net today). The `mpsc` channel in `net_tx()` keeps job closures alive until the net thread drains
them; nothing caps that queue, so a burst of dropped requests retains all closures until thread exit.

**Verified clean:** no QML reference cycles (`FeedJson`/`ModelSync`/`AccountOverrides` are stateless
singletons; `Sidebar.expandedById`/`MessageList.scrollMemory`/`headersInfo`/`currentMessage` are plain JS
objects; `backend` references point at objects owned by `Main.qml`). No binding loops (header/spacer,
`fitLayout`, filter and scroll-memory paths all resolve without re-entering a binding). `Connections`
targets and handler names are right (`onJob_finished`/`onJob_progress`/`onUndo_available`). Timers are
children of their owner. Delegate models use `required property`. `undo_available`/`job_finished`/
`job_progress` are emitted only from `qt.queue` closures. One nuance: `bridge/bulk.rs:194-195,246-247`
emits `undo_available` synchronously on the GUI thread inside the invokable, safe only because the row-level
emits elsewhere are deferred with `Qt.callLater`.

---

## E. Native Android + FFI — `crates/mailffi/src/`, `android/`

### E1 · critical · `auth_vault.json` is not excluded from device-to-device transfer
`android/app/src/main/res/xml/data_extraction_rules.xml:4-8`

```xml
<data-extraction-rules>
    <cloud-backup>
        <exclude domain="file" path="auth_vault.json" />
    </cloud-backup>
</data-extraction-rules>
```
On API 31+ a `<device-transfer>` section with no `<exclude>` transfers every app-private file, so the
plaintext secrets file (`mailcore/src/auth.rs:168-183`, written by `init` into `filesDir`) moves to the new
device.
**Fix:** add `<device-transfer><exclude …/></device-transfer>` with the same paths.

### E2 · high · the whole local mailbox is in cloud backup
`android/app/src/main/res/xml/backup_rules.xml:4`, `AndroidManifest.xml:38`

```xml
<full-backup-content>
    <exclude domain="file" path="auth_vault.json" />
</full-backup-content>
```
`android:fullBackupContent` with no `<include>` means "everything except these", so `mailclient.sqlite`
(cached subjects, senders, snippets, and full bodies/attachments once read) and `filesDir/crashes/*.log` are
uploaded, despite the comment claiming "Mail itself stays on the server".
**Fix:** also exclude `mailclient.sqlite` (+ `-wal`/`-shm`) and `crashes/`, or set `android:allowBackup="false"`.

### E3 · medium · `MailNative.init()` opens SQLite and migrates on the UI thread
`ui/shell/MailShell.kt:291-294` → `ensureInit` → `init()` → `use_data_dir()` + `shared_db()` →
`mailcore::Db::open` = "Open (creating parent dirs) and migrate to the current schema"
(`mailcore/src/db/mod.rs:35-43`). When the UI process is the first into the library, a post-update migration
of a large cache runs on the main thread.
**Fix:** call `ensureInit` from `MailApplication.onCreate` on a background thread (also fixes E19).

### E4 · low · `spawn` → `forward_busy` invokes Java from the caller's thread
`crates/mailffi/src/net.rs:259-260`

```rust
#[cfg(target_os = "android")]
crate::android::forward_busy(&queued_kind, &queued);
```
`ReaderFiles.ensureBytes` calls `MailNative.downloadAttachments(...)`, and `awaitFinished` runs `queue()`
inside `withContext(Dispatchers.Main)` (`ui/state/MailState.kt:499-505`), so the `queued` event re-enters Java
(`JobEvents.onJobEvent`) on the main thread from inside a native frame. Legal today only because the
subscriber just `scope.launch(Dispatchers.Main)`.
**Fix:** run `queue()` on an IO dispatcher in `awaitFinished`.

### E5 · low · every job event attaches and detaches the `mailclient-net` thread
`crates/mailffi/src/android.rs:1346` and `:247` — `self.vm.attach_current_thread(|env| …)`. The `jni` crate's
own docs warn against scoped attachment for a long-lived thread; each `queued` + `finished` pair pays a full
JVM attach/detach.
**Fix:** hold a permanent `AttachGuard` for the net/monitor thread.

### E6 · low · `uid < 0` is silently clamped to UID 0 instead of rejected
`crates/mailffi/src/android.rs:379,401,440,515,628,647,680,…`

```rust
uid.max(0) as u32,
```
A Kotlin-side `-1` ("no message") becomes a real operation on UID 0 rather than an error, and the `uid: i32`
JNI type truncates UIDs above `Int::MAX` (same class as D3).
**Fix:** `u32::try_from(uid).map_err(...)` for `uid >= 0`, error otherwise.

### E7 · low · malformed `permanent_json` is swallowed into the most destructive answer
`crates/mailffi/src/android.rs:581-583`

```rust
let permanent: Vec<Option<bool>> =
    serde_json::from_str(&string(env, &permanent_json)?).unwrap_or_default();
```
`delete_prompt` treats an empty slice as `permanent = true` (`mailcore/src/undo.rs:77`), so a bad payload
silently flips the UI to "delete permanently, always confirm" with no clue why.
**Fix:** `?` the parse instead of `unwrap_or_default()`.

### E8 · medium · attachment finish events have no correlation key
`ui/reader/ReaderFiles.kt:63-66`

```kotlin
if (done != null && !done.first) throw DownloadFailed(done.second.ifEmpty { "Download failed" })
```
`done` is simply "the next `Attachments` finish", which may belong to a *different* message's job. The user sees
an unrelated error string for an attachment that is fine, and `MAX_FINISH_WAITS = 5` waits (up to 10 minutes)
can be burned on other messages' jobs.
**Fix:** include the target (`folder_id`, `uid`) in the event or in a `MailNative.attachmentsResult(folderId, uid)`
read, and key the waiter on it.

### E9 · low · `createFolder`'s waiter can be fired by an unrelated `Folders` job
`ui/state/MailStateFolders.kt:246` + `ui/state/MailState.kt:457` —
`finishWaiters.remove(kind)?.forEach { it(ok, e.optString("status")) }`. A "Refresh" finish that lands between
waiter registration and queue satisfies the create, so `FolderManagerScreen` shows the refresh's status (or an
unrelated failure) and clears it. Same class as E8.
**Fix:** add a correlation token to the event and match it.

### E10 · low · `spawn_flag_push` un-deduped; `spawn_push_after_grace` spawns an OS thread per action
`crates/mailffi/src/net.rs:272-294` — `std::thread::spawn` + `sleep(grace+1)` per undoable action
(`api/mutate.rs:106,194`). N rapid archives = N sleeping threads plus N queued IMAP pushes.
**Fix:** coalesce on the existing in-flight table (same fix as D8/B6).

### E11 · low · `MailNotifier.onMailChanged` is a single global slot cleared by composition
`ui/shell/MailShell.kt:353-359`

```kotlin
onDispose { MailNotifier.onMailChanged = null }
```
A background check that finishes while the shell is momentarily disposed hits `MailNotifier.kt:66`
(`if (plan.getString("action") == "foreground") onMailChanged?.invoke()`): the notification is suppressed
*and* the list is not refreshed, so new mail is invisible until the next resume/event.
**Fix:** keep a process-level "cache changed" flag the shell drains in `ensureInit`.

### E12 · medium · `refreshSidebarRows()` runs a Rust SQL aggregate on the main thread
`ui/state/MailStateFolders.kt:116-125`, called at `:99` (inside `withContext(Dispatchers.Main)`) and `:112`
(tap handler)

```kotlin
internal fun MailState.refreshSidebarRows() {
    runCatching { parseSidebarRows(MailNative.sidebarRowsJson(id, expanded)) }
```
`sidebar_rows_json` → `messages::counts_by_account` (a `GROUP BY` over the messages table) +
`folders::list_by_account` (`mailcore/src/feed.rs:163-166`). Every finished job event
(`onJobEvent` → `loadFolders()`) and every expand/collapse tap does this on the UI thread, while every other
DB read in the same file is deliberately in `io { }`.
**Fix:** build the JSON before the `withContext(Dispatchers.Main)`, assign state inside it.

### E13 · medium · the reader's reply strip overflows at 360dp / 150 % text scale *(suspected)*
`ui/reader/ReaderScreen.kt:291-299`

```kotlin
Row(modifier = Modifier.fillMaxWidth().height(48.dp), horizontalArrangement = Arrangement.SpaceEvenly) {
    ReplyAction(R.drawable.ic_reply, "Reply") { … }
    ReplyAction(R.drawable.ic_reply_all, "Reply all") { … }
    ReplyAction(R.drawable.ic_forward, "Forward") { … }
```
Three icon+label `TextButton`s with no `Modifier.weight` sum to ~330dp at 100 % scale and ~420dp at 150 %;
`Row` does not wrap, so the last action is squeezed to zero width and becomes unreachable (measured, not run).
**Fix:** `Modifier.weight(1f)` on each `ReplyAction` (or icons only / a `FlowRow` above a scale breakpoint),
and `heightIn(min = 48.dp)`.

### E14 · medium/low · JNI calls inside `remember` blocks (side effects in composition)
Three sites:
- `ui/composer/ComposerScreen.kt:166-170` — `remember(editorBody) { … MailNative.editorDocument(…) }` builds
  the entire editor HTML document (body embedded, fresh nonce) **on the main thread**; the same build is done
  on `Dispatchers.IO` in `MailWebView.kt:128-133`.
- `ui/reader/ReaderScreen.kt:451-464` — `remember(m, dark, originalColors, scheme) { pagePaint(…) }` → two
  JNI calls.
- `ui/list/ListScreen.kt:111-113` — `remember(state.folders) { state.folders.associate { it.id to state.deletePrompt(…) } }`.
**Fix:** compute in a `LaunchedEffect` on IO, hold the result in state.

### E15 · low · `StatusStrip`'s tap line is 40dp tall
`ui/shell/ShellBars.kt:237-257` — `.height(40.dp)` with the clickable line inside; below the 48dp touch-target
floor AGENTS.md requires.
**Fix:** `heightIn(min = 48.dp)`.

### E16 · low · stale `@SuppressLint` and unset `mixedContentMode`
`ui/reader/MailWebView.kt:65` vs `ui/composer/ComposerEditor.kt:163` — `MailWebView` sets
`javaScriptEnabled = false` (`:143`), so the suppression is stale and hides accidental future changes.
`ComposerEditor` legitimately enables JS with `addJavascriptInterface`; it is safe only because
`mailcore::compose::editor::document` embeds `draft_editor_html`, which passes through
`html::sanitize_for_send` (allow-list, drops `script`), plus a nonce CSP, `allowFileAccess=false`,
`blockNetworkLoads=true` — none of which is asserted locally.
**Fix:** remove the stale annotation; set `mixedContentMode` explicitly and comment the `MCHost` contract.

### E17 · low · `usesCleartextTraffic="true"` app-wide with no `networkSecurityConfig`
`AndroidManifest.xml:40` — the comment says cleartext is "opt-in per account", but the flag is global: with
`load_remote_images` on, an `http://` image from a plaintext-configured account's mail is fetched in the clear
by the reader WebView regardless of the account's TLS choice.
**Fix:** scope cleartext with `networkSecurityConfig` to the configured hosts, or reword the comment.

### E18 · low · `Scaffold` gets `safeDrawingPadding()` while its default `contentWindowInsets` also applies
*(suspected)* `ui/shell/MailShell.kt:439-443` — the reader's own `Scaffold` opts out with
`contentWindowInsets = WindowInsets(0)`; the shell's does not, so the content column may get the system-bar
inset twice (a large top gap). Verify on device.
**Fix:** `contentWindowInsets = WindowInsets(0, 0, 0, 0)` on the shell Scaffold.

### E19 · low · several screens never call `MailNative.ensureInit`
`ui/contacts/ContactsScreen.kt:128,137`, `ui/outbox/OutboxScreen.kt:93`, the folder screens and
`MaintenanceSection`/`BackgroundStatus`. Only `MailShell` and the background components
(`MailCheckWorker`, `MailPushService`, `MailSchedule`, `MailActions`) initialise the core. Today the shell
always runs first, but any of these reached before the shell's `DisposableEffect` (or a future notification deep
link into a full-page route) would execute against the fallback `db_path()`
(`mailcore/src/db/mod.rs:18-25`), which on Android resolves to a relative path under `/` and fails.
**Fix:** initialise in `MailApplication.onCreate`.

### E20 · low · notification signature format re-implemented in Kotlin
`MailNotifier.kt:118` duplicates `mailcore/src/sync/background/notify.rs:123-125`

```kotlin
out.put(tag, "$title\n$body")
// rust: pub fn signature_of(title: &str, body: &str) -> String { format!("{title}\n{body}") }
```
The two must match byte-for-byte or every posted notification reads as "changed" (or never changes).
**Fix:** expose `MailNative.signature(title, body)`, or have `plan()` return the per-tag signatures.

### E21 · low · the "Similar to: …" sentence is built twice, with different empty-subject fallbacks
`ui/state/MailStateSearch.kt:79` vs `crates/mailapp/qml/MessageList.qml:773`

```kotlin
similarLabel = "Similar to: ${subject.ifEmpty { "this message" }}"
// qml: text: qsTr("Similar to: %1").arg(root.similarSubject)
```
Qt shows nothing for an empty subject, Flutter "(no subject)" — so the same data renders differently per
frontend, which is exactly what `mailcore` is meant to prevent. Rust function: `mailcore::similar::target_subject`.
**Fix:** `mailcore::similar::chip_label(db, …) -> String` returning the whole sentence.

### E22 · low · Kotlin re-parses the core's `ReadTarget` JSON to find `account_id`
`android/.../MailActions.kt:33` — `MailFlagWorker.enqueue(app, JSONObject(target).getLong("account_id"))`.
`ReadTarget` is a Rust struct (`notify.rs:132-135`); the WorkManager enqueue only needs the account id, so a
frontend must know an internal field name.
**Fix:** `MailNative.markReadAccount(target): Long`, or have `markRead` return `(report, account_id)`.

### E23 · low · `SHARED-CORE.md` items 8 and 2 are open or stale
- `ui/contacts/ContactsScreen.kt:77-87` (`Candidate.reasonText`) — Kotlin wording of the core's machine
  reasons, no Rust function exists (reasons come from `contacts::cleanup_candidates_json`).
  **Fix:** a `mailcore` label map.
- `ui/reader/ReaderScreen.kt:317-321` (`canToggleColors`) — the "when the colours toggle shows" decision,
  Kotlin-only, as listed.
- `ui/settings/SettingLabels.kt:10-44` and `AccountSetupScreen.kt:532-536` (`securityLabel`) — a 10-key plus
  3-key value→words map in Kotlin, kept in step with the QML twins by convention only.
  **Fix:** promote the label maps into `mailcore::store::settings` next to `choices`.
- §2 ("Sync-on-resume gap … lives in Kotlin (`MailState.RESUME_SYNC_GAP_MS`)") is **stale**: the constant no
  longer exists; it is now `MailNative.resumeSyncDue` → `mailcore::sync::resume::resume_sync_due`.
  **Fix:** delete the stale entry.

### E24 · informational · `ui/folders/FolderIcon.kt:17-25`
Maps the core's folder role to a drawable — declared frontend-only in SHARED-CORE, so no action; listed for
completeness.

### E25 · verified clean · no leaked parent `Global<JObject>`
`pushStart` (`android.rs:297-310`) drops the old monitor and its `Arc<KotlinListener>` before installing the new
one, and `MailPushService.onDestroy` calls `pushStop()`, so the service's global ref is released.

**Also verified clean in `android.rs`:** no panic path across the JNI boundary was confirmed, no local-reference
table overflow (no long JNI loops without `DeleteLocalRef`), no `GetStringUTFChars` read-after-release.

---

## F. Cross-cutting duplication (AGENTS.md §1 / §5 core-first)

| # | Logic | Qt side | Android side | Belongs in |
|---|---|---|---|---|
| F1 | notification signature `"<title>\n<body>"` | — | `MailNotifier.kt:118` | `sync::background::notify::signature_of` (exists, E20) |
| F2 | "Similar to: …" sentence | `MessageList.qml:773` | `MailStateSearch.kt:79` | `mailcore::similar::chip_label` (E21) |
| F3 | undo batch splitting | `Main.qml:1456-1464` JS | — | `mailcore` (D11) |
| F4 | pending-open payload decode | `Main.qml:777` JS | `MailActions.kt:33` | `mailcore` (D11, E22) |
| F5 | attachment filename decode | `MessagesView.qml:376-384` JS | — | `mailcore::paths` (exists, D11) |
| F6 | file:// URL building | `MessagesView.qml:170-181` JS | — | `mailapp::bridge::messages::files` (exists, D11) |
| F7 | account/security value → words | `AccountSetup.qml` / `Settings.qml` | `SettingLabels.kt:10-44`, `AccountSetupScreen.kt:532-536` | `mailcore::store::settings::choices` (E23) |
| F8 | cleanup-candidate reason wording | `Contacts.qml` | `ContactsScreen.kt:77-87` | `mailcore::store::contacts` (E23) |

---

## Verification methodology

- **Read:** every file in `crates/mailcore/src`, `crates/mailapp/src`, `crates/mailapp/qml`,
  `crates/mailffi/src` (except generated `frb_generated.rs`), and `android/app/src/main` Kotlin,
  plus the four manifest/backup XML files and the root docs.
- **Reproduced (`[verify]`):** C1 (real `domain_label` + `label[..4]`), C2 (exact algorithm, timing table),
  A1/A3/A11 (synthetic `version=21` DB, `schema_meta` variants), A2 (two-connection deferred-tx upgrade),
  A4 (32 764-variable statement), A7 (`attachment_has_data` on a missing id), A8 (`to_addrs='not json'`),
  A18 (`sqlite_master` diff + grep for `select *`), B1 (lettre `Timeout(None)` semantics).
- **Not run:** `cargo fmt`/`clippy`/`cargo test`, `scripts/qml-check.sh`, `./build.sh --android`.
  Run the applicable one before closing each item.
- **Assumptions to confirm:** D3/C3 uid `i32` width on a real large mailbox; E13 and E18 layout measurements
  on a device; B11 partial-move behaviour on a real server.
