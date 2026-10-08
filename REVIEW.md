# Codebase Review — Findings Backlog

Read-only review of the whole repo (mailcore, mailapp/Qt, mailffi, native Android), 2026-10-08.
Work top to bottom; each item is self-contained so they can be picked out of order.

- **Severity:** `critical` (crash / data loss / secret exposure), `high` (wrong results, hangs, timeouts),
  `medium` (latent or narrow), `low` (polish / hygiene).
- **Status tags:** `[verify]` = I reproduced it by running code; `[corrected]` = an earlier draft of this
  review got it wrong and this is the fixed version; `[fixed]` = fixed in the repo, with the commit noted in
  the item; `[removed]` = withdrawn, with the reason kept so the ids stay stable for cross-referencing.
  Untagged = read from source, not executed. **Applied so far:** E1, E2 (`d3ef68b`), C1 (`1dac9e7`),
  C7 (`11137f4`), D1 (`74d3a4d`), D6 part 1 (`c3d90fb`), B1 (`bb9edb6`), C2 (`946e971`),
  A2 (`5e07cab`), A4 (`f4ff35e`), C15 (`9698314`). See §Applied for what each did and what it did not.
  **Applied together in one later commit** (the one that added this line): A7, A11, A13, A17 (dead attachment delete only), C8, C9, C14, plus
  follow-up corrections to A2, A4, B1, C2, C15 and D6 from a validation pass over those six commits.
- **AGENTS.md §7** governs closing any item: Rust → `cargo fmt --check` + `cargo clippy -p mailcore -- -D warnings`
  + `cargo test -p mailcore`; QML → `scripts/qml-check.sh`; Android → `./build.sh --android`.
- **§0** records the second-pass audit of this document. Read it first if you are trusting these findings.

---

## 0. Audit of this review (second pass)

A second reviewer challenged 10 items and several severity ratings. I re-read the code for each. Outcome:
6 items withdrawn, 4 rewritten, 8 severities changed, 9 suggested fixes replaced because the original
would have caused damage. Reference errors corrected. Nothing in the repo was edited.

**Withdrawn outright (not bugs):**

| Item | Why |
|---|---|
| A16 | Pre-v12 every stored path was raw server form, so decoding was correct. The proposed guard (`encode == path`) is true for pre-v12 data too, so the rename still happens. |
| B8 | `messages::delete_by_folder` runs at `engine/sync.rs:81`, **before** `local_uids` is read at `:141`. After a validity change the set is empty, so step 3's `older_existing` is empty and the stale modseq is never used. |
| B11 | `session.uid_move(&clean, dest_path).await?` at `engine/mutate.rs:107` returns **before** `delete_many_by_uids`, so rows are never deleted after a failed move. Replaced by B11' (the two real bugs next to it). |
| B14 | `checkin()` does `drop(pool); … s.disconnect()` (`pool.rs:101-112`) — the lock is released first, and `disconnect()` is only `session = None`. The `Drop` impl never holds the lock. |
| C13 | `mime.rs:293-311`: the comment above the dead branch says "A ZIP-subtype sniff (docx/xlsx/…) is more specific than a plain ZIP or generic header, but **never overrules a different specific type**". Returning `None` both times is the documented decision. |
| E24 | `ui/folders/FolderIcon.kt` is not in `SHARED-CORE.md` at all, so there was no violated exemption. |

**Severity raised (the critique understated these):** C2, C6, C7, C15, D1, D6.
**Severity lowered (I overstated these):** A1, A4, E1, D10, B17, B19.
**Fixes replaced because the original was harmful:** A1, A7, A10, A12, A15, B2, C7, C14.
**Merged:** D17 into D5 (same thing), B6 into D8 (same thing).
**Fixed reference errors:** `MessagesView.qml` does not exist (those helpers live in `MessageView.qml` and
`Composer.qml`); `store/undo.rs` → `undo.rs`; `bridge/bulk.rs` → `bridge/messages/bulk.rs`;
`net.rs:280-287` → `net.rs:288-294`; `contacts_json` is at `bridge.rs:140`; methodology "D3/C3" → "D3/E6".

### Corrected fix order

| # | Item | Why now | |
|---|---|---|---|
| 1 | ~~**C1** `badge.rs:76` byte-index panic~~ | ~~one malicious sender crashes folder listing on every frontend~~ | **done** — `1dac9e7` |
| 2 | ~~**E2** backup excludes~~ | ~~whole local mailbox (bodies, attachments) uploaded to cloud backup~~ | **done** — `d3ef68b` |
| 3 | ~~**C7** `is_public_remote` IPv6~~ | ~~every bracketed IPv6 host, incl. `[::1]`, passes as public; metadata IP reachable~~ | **done** — `11137f4` |
| 4 | ~~**D1** `expect` + latched `busy`~~ | ~~a failed net-thread bootstrap aborts or bricks every later job~~ | **done** — `74d3a4d` |
| 5 | ~~**D6** GUI-thread image read~~ | ~~whole file read before the size check, on the GUI thread~~ | **done (1 of 2)** — `c3d90fb` |
| 6 | ~~**B1** SMTP has no timeout~~ | ~~one dead SMTP host hangs the net thread forever~~ | **done** — `bb9edb6` |
| 7 | ~~**C2** quadratic entity decode~~ | ~~~9 s CPU per crafted mail, on both `sanitize` and `html_to_text`~~ | **done** — `946e971` |
| 8 | ~~**A2** deferred-tx upgrade race~~ | ~~intermittent "database is locked" on attachment save~~ | **done** — `5e07cab` |
| 9 | ~~**A4** unbounded `uid in (?,…)`~~ | ~~bulk action on >32 766 UIDs fails; self-heals but the action is lost~~ | **done** — `f4ff35e` |
| 10 | ~~**C15** calendar `rposition`~~ | ~~quadratic `END` lookup turns a 25 MB `.ics` into a hang~~ | **done** (reason corrected) — `9698314` |

---

## Applied

What each landed fix actually changed, and what it deliberately left alone. Read
the "not fixed" column before assuming a finding is closed.

| Item | Commit | What it fixed | Not fixed / needs a decision |
|---|---|---|---|
| **E1** | `d3ef68b` | Added `<device-transfer>` to `data_extraction_rules.xml`, so the vault file stops moving device-to-device | — |
| **E2** | `d3ef68b` | `mailclient.sqlite` (+ `-wal`/`-shm`, WAL matters) and `crashes/` excluded from cloud backup, in **both** rule files | Nothing restorable is left, so `allowBackup="false"` is the cleaner equivalent — a decision, since it also drops the scheduling prefs from a future restore |
| **C1** | `1dac9e7` | `label.get(..4)` instead of `len() >= 4 && label[..4]`, so a non-ASCII sender address cannot panic the badge | — |
| **C7** | `11137f4` | All IPv6 URLs, `169.254/16` (metadata), `100.64/10`, `0/8`, `198.18/15`, `240/4` and numeric `inet_aton` shorthands are blocked; a trailing root dot no longer hides `.local` or an address | Hex (`0x7f000001`) and octal forms still fall through to the hostname branch — deliberate, widening "digits and dots only" to "looks numeric" would false-positive on real hostnames |
| **D1** | `74d3a4d` | Runtime built inside the spawned thread (no GUI-thread abort); `busy` latched only after a job is queued; a dead net reported distinctly from busy | The `queued` `Err` tail branch still only logs, since with a live thread holding its receiver the send cannot fail |
| **D6 part 1** | `c3d90fb` | `image_data_url` stats before reading, so a huge file is refused without loading it; the error reports the real size | **The rest of D6 is still open.** The keyring D-Bus round trip, draft/forward staging (temp writes + a `read_dir` walk) and the maintenance `cleanup_temp`/stats all still run on the GUI thread. Moving any of them needs an async QML contract (a signal path instead of a returned status string) — an API decision in both frontends, not a local fix. *Follow-up:* the second test was named `a_file_that_grows_past_the_limit_is_still_refused` but only inlined a 64-byte image; renamed to what it does. The grow-between-stat-and-read cap has no test — it needs a race the test cannot stage |
| **B1** | `bb9edb6` | 30 s timeout on the submit transport and the DSN connection | `spawn_blocking` was **not** done: the net thread runs one job at a time (`rx.recv()` then `rt.block_on(job)` in `bridge/worker.rs`), so the next job waits for this one whether the SMTP call blocks the runtime or a blocking-pool thread. The timeout is the fix. 30 s is half lettre's own 60 s default and applies per read/write, so a server that scans a large mail for long after `DATA` could now time out. *Follow-up:* the stale "sends keep the transport default" doc line on `test_connection` now names `SUBMIT_TIMEOUT` |
| **C2** | `946e971` | Bounded scan for the closing `;`; 11.5 s → 12.4 ms on the same 512 KB input | The 9.32 s in the item's table and the 11.49 s here are two separate runs of the same input. *Follow-up:* the code comment said "~9 s", and the boundary test's comment wrongly claimed both cases fail if the window widens (only the 25-byte one does; the 24-byte one guards narrowing) |
| **A2** | `5e07cab` | Transaction is `IMMEDIATE` | **Corrected in the follow-up:** `5e07cab` also moved the read *before* the `BEGIN`, which opened a new race — two writers for the same message could both see the old rows and insert duplicates, or update a row the other had just deleted. With `IMMEDIATE` the read needs no upgrade, so it is back inside the transaction. The test is a mechanism test, not a regression guard (the interleaving is inside the function); it now asserts the error really is `SQLITE_BUSY_SNAPSHOT` (517) instead of discarding it, and compares against `SCHEMA_VERSION` instead of a hard-coded `"23"` |
| **A4** | `f4ff35e` | `uid in (?)` chunked at 900, inside `execute_over_uids` so all three callers are covered | Caller leading bindings became a borrowed slice — the old `Vec<Box<dyn ToSql>>` could not be repeated per chunk. *Follow-up:* `UID_CHUNK` had been inserted mid-sentence into `execute_over_uids`'s doc comment, splitting it across both items — restored. More than one chunk now runs in an `IMMEDIATE` transaction (skipped when the caller already has one open), so a failing chunk no longer leaves the earlier ones applied |
| **A7** | follow-up | `attachment_has_data` uses `.optional()` and returns `NotFound` for a missing row, never `Ok(false)` (which would start a download) | — |
| **A11** | follow-up | `ensure_schema` refuses a stamp newer than `SCHEMA_VERSION` before touching anything, instead of rewinding it | Refusing means an older build cannot open the file at all; that is the intended trade |
| **A13** | follow-up | Dead `account_form::test_connection` deleted; its two tests retargeted at `prepare_connection_test`, the live path; its password-fallback doc moved there | — |
| **A17** (one bullet) | follow-up | Dead `delete_attachments_for_message` and its test lines deleted | The other three A17 bullets are open |
| **C8** | follow-up | `<!-->` and `<!--->` close at their `>` | — |
| **C9** | follow-up | `<?` ends at the first `>`, as HTML parses it; `<?xml … ?>` ends at the same place | — |
| **C14** | follow-up | `s.get_mut(..1)` instead of `s[..1]` in the vCard label | — |
| **C15** | `9698314` | Component-stack depth capped at 32; deeper input is refused | **Reason corrected: not quadratic.** Measured linear (8× depth → ~8× time, 4.1 s for 13 MB), so it is a memory/CPU bound, not a blowup. The first draft and the second reviewer both said "quadratic/hang" and both were wrong. *Follow-up:* the comment at the cap check still said "quadratic in the depth" — corrected |

## A. `mailcore` persistence layer — `db/`, `store/`, `models.rs`

### A1 · medium · version stamp collapse bricks open forever `[verify]` `[corrected]`
`db/migrations.rs:132-151`

```rust
let current: u32 = conn.query_row(…, |row| { let v: String = row.get(0)?; Ok(v.parse::<u32>().unwrap_or(0)) }).unwrap_or(0);
if current == 0 {
    conn.execute_batch(SCHEMA_FULL)?;
    conn.execute("insert into schema_meta (key, value) values ('version', ?1)", …)?;   // PK clash
```
A **present but unparseable** `schema_meta.value` collapses to `0`, so an existing DB takes the
fresh-install path and the plain `INSERT` hits the primary key. `[verify]` `''`, `'0'`, `' 23'`, `'v23'`,
`'-1'` and `'999999999999'` all yield `UNIQUE constraint failed: schema_meta.key`, on every later open.
A **missing** row is handled correctly.
Severity lowered from critical: it needs a hand edit or corruption of that one value, not ordinary use.

**Fix (the obvious upsert is wrong):** an upsert would stamp the current version onto a database of
unknown version and silently skip the migrations it still needs. Distinguish the two cases instead —
read the row as `Option`:

```rust
let raw: Option<String> = conn.query_row("select value from schema_meta where key = 'version'", [], |r| r.get(0)).optional()?;
let current: u32 = match raw.as_deref().map(str::trim).map(str::parse::<u32>) {
    None => 0,                                        // no row: fresh install
    Some(Ok(v)) => v,
    Some(Err(_)) => return Err(… "corrupt schema_meta version, refusing to migrate"),  // present but unreadable
};
```

### A2 · high · deferred transaction reads before it writes → `SQLITE_BUSY` — **FIXED** `[fixed]`
`store/messages/attachments.rs:50-51`

```rust
let tx = db.conn().unchecked_transaction()?;
let mut existing = list_attachments(db, message_id)?;   // read snapshot taken here
… tx.execute("update attachments set data = …")
```
`unchecked_transaction()` is a deferred `BEGIN`; the read already holds a snapshot, so if any other
connection commits in between (GUI thread + net thread in `mailapp`; the FRB pool in `mailffi`; the
`--sync-once` CLI that `queue.rs:20` documents as sharing the file) the write upgrade fails immediately —
`busy_timeout` does not cover snapshot-upgrade. `[verify]` two connections, deferred tx + prior read →
`database is locked`; a write-only deferred tx succeeds.
**Fix:** `TransactionBehavior::Immediate`, with the read inside it. *(Corrected: "or read `existing` before
`BEGIN`" was wrong — it trades the upgrade failure for a lost-update race between two writers of the same
message. `5e07cab` did both; the read is back inside the transaction in the follow-up commit.)*

### A3 · medium · failed migration steps still stamp success `[verify]` — confirmed
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

### A4 · medium · unbounded `uid in (?,?,…)` fails at 32 766 UIDs — **FIXED** `[fixed]`
`store/messages/flags.rs:127-128`

```rust
let placeholders = vec!["?"; clean.len()].join(",");
let sql = format!("{sql_head} uid in ({placeholders})");
```
`[verify]` SQLite's `SQLITE_MAX_VARIABLE_NUMBER` is 32,766; `execute_over_uids` passes
`sql_head`'s leading params too, so the real threshold is a little lower and path-dependent (I measured
32,764 on the delete path). The **correct** claim: any selection over ~30 k UIDs fails, and `execute_over_uids`
has no chunking while `engine/sync.rs:136` does chunk.

Entry points that reach it: `bridge/messages/bulk.rs` `flag_hits`/`queue_hits` → `bulk::set_read`/`set_starred`
→ `store/messages/flags.rs:89`, and `sync/imap/engine/mutate.rs:108,124` (`move_uids_to`, `purge_uids`),
which get an **unchunked** group from `push_due_moves`. There is no cap on search hits or on a select-all
selection, so this is reachable; a 50-row UI selection is not the only way in.
Severity lowered from high: the next sync reconciles the untouched rows, so the local state self-heals —
the user's action is silently lost, which is why it still matters.
**Fix:** chunk `uids` at ≤900 inside `execute_over_uids` so every caller is covered.

**Fixed in `f4ff35e`.** Chunking sits inside `execute_over_uids` so all three callers are covered at once.
The caller-side leading bindings became a borrowed slice of `&dyn ToSql`, because the old
`Vec<Box<dyn ToSql>>` could not be repeated per chunk — `now()` is now bound once per caller instead of
being built inside the array literal. One test with a 40 000-uid list and rows on UID 900 (the chunk
boundary): it panics when the chunking is removed, so it is a real guard rather than a restatement.

### A5 · medium · migration repair: no transaction, `prepare` inside the loop
`db/migrations.rs:382-408` — `migrate_cid_inline_attachments` re-prepares the statement per message
(N+1) and writes unbatched with no transaction, so a crash leaves a half-repaired attachment set that
(per A3) is never re-run.
**Fix:** prepare once outside the loop; run in one `unchecked_transaction()`.

### A6 · medium · every error becomes "not found" `[corrected]`
`store/queue.rs:120-128`

```rust
.query_row(&format!("select {COLS} from send_queue where id = ?1"), [id], row_to_queued)
.map_err(|_| StoreError::NotFound(format!("queue entry {id}")))
```
A locked DB, a corrupt row or an I/O error is misreported as `NotFound`, so callers cannot distinguish
"row is gone" from "the database failed" and keep retrying a permanently broken row. Every other store
uses `.optional()?`. Severity lowered slightly: only affects the send queue, and the retry is bounded.
**Fix:** `.optional()?.ok_or_else(|| StoreError::NotFound(…))`.

### A7 · medium · `attachment_has_data` leaks a raw no-rows error — **FIXED** `[fixed]`
`store/messages/attachments.rs:173-182` — `[verify]` `attachment_has_data(db, 4242)` returns
`database error: Query returned no rows`; `crates/mailffi/src/api/attachments.rs:27` and
`crates/mailcore/src/compose/forward.rs:105` surface that string to the user where "attachment not found"
was meant.
**Fix (do *not* return false):** a `false` here tells the caller the attachment is not cached yet, so it
would start a download for an attachment that does not exist. Use
`.optional()?.ok_or_else(|| StoreError::NotFound(format!("attachment {id}")))`.

### A8 · medium · `unwrap_or_default()` hides column corruption `[verify]` — confirmed, severity trimmed
`store/messages.rs:47-49,60` and `store/queue.rs:75`

```rust
to_addrs: json_vec(&to).unwrap_or_default(),
```
`[verify]` writing `to_addrs = 'not json'` then reading back yields `to_addrs=[]` with no log line. In
`queue.rs` (`envelope_to: json_vec(&to_raw).unwrap_or_default()`) a corrupt envelope row is presented as
having zero recipients and is claimed/submitted as such. Same at `contacts.rs:543`
(`serde_json::to_value(c).unwrap_or_default()` silently drops a contact from `contacts_json`) and `choices.rs:81`.
Reachable only via a hand-edited or corrupt DB, hence medium rather than high.
**Fix:** `warn!` on parse failure; treat an unparseable `envelope_to` as a hard error.

### A9 · low · "transactional" deletes that are not — confirmed as written
`store/contacts.rs:449-463` — `delete_many` is documented/aliased as transactional but issues one implicit
transaction per address; a mid-list failure leaves a partial delete. Same in `pending_moves::record_failure`/
`remove` (`:159-178`) and `bridge/messages/bulk.rs` `set_read` (`store/messages/bulk.rs:44-48`).
**Fix:** wrap in `unchecked_transaction()` or collapse to `where address in (?,…)`.

### A10 · low · multi-statement mutations outside the tx helper `[corrected]`
`store/settings.rs:266-268` (`set_pending_open` calls `set` twice), `:394-395` (`set_sort` calls `set` twice),
while `set_many` (`:175-202`) does it in one transaction.
**Fix (partly blocked):** `set_many` rejects any key without an entry in `defaults()`
(`settings.rs:176-180`), and `PENDING_OPEN_ACCOUNT_ID`/`PENDING_OPEN_FOLDER` (`settings.rs:106,108`) have
none. So `set_pending_open` cannot use it until those keys are added to `defaults()` (they are take-once,
with no default). **`set_sort` can be converted today.**
**Fix for `set_pending_open`:** wrap the two `set` calls in one `unchecked_transaction()`.

### A11 · low · a *newer* version stamp is silently rewound — **FIXED** (refuses) `[fixed]`
`db/migrations.rs:327-332` — `[verify]` a DB stamped `99` opens "successfully" and is rewritten to the current
version; the newer build then re-applies its own migrations over this build's schema.
**Fix:** log loudly (or refuse) when `current > SCHEMA_VERSION`.

### A12 · medium · `save_edit` has no keyring compensation `[corrected]`
`store/account_form.rs:404-409` — `secrets.save(...)` lands **before** `accounts::update_connection(...)`,
whereas `create_new` (`:429-434`) deletes the orphaned vault entry when the row update fails. A failed
`save_edit` therefore leaves *new* passwords next to *old* host/user: an account that cannot connect, with
no way for the user to tell which half is stale.
**Fix (the obvious delete is wrong):** the vault key belongs to the **existing** account, so deleting it on
failure wipes the credentials of an account that is still configured and working. Instead either (a) update
the row first and only write secrets once it succeeds, or (b) re-save the previous secrets on the failure
path. (a) is the smaller change.

### A13 · low · dead `async fn` holding `&Db` across `.await` — **FIXED** (deleted) `[fixed]`
`store/account_form.rs:240-245` — the future is `!Send` (`&Db` is `!Send`). Both adapters deliberately avoid it
(`crates/mailffi/src/api/accounts.rs:75-96` documents "split-phase so the future stays `Send`"), leaving this
wrapper with only its own tests as callers and the FRB pool unavailable to it.
**Fix:** delete it, or drop the `db` parameter.

### A14 · low · silent truncation casts — confirmed
`store/accounts.rs:17,20`, `store/folders.rs:18-21`, `store/messages.rs:42,61`, `store/queue.rs:72` —
`row.get::<_, i64>(5)? as u16` / `as u32` / `as u64` on every port / uid / count. A port stored as 70000
reads back as 4464; a negative `size` reads back as 1.8e19.
**Fix:** `u16::try_from(v).unwrap_or_default()` or a checked conversion with a warning.

### A15 · medium · full-table scan + full Rust sort on every keystroke `[corrected]`
`store/contacts.rs:399-415` — `suggest(db, prefix, 10)` reads **every** contact and fuzzy-scores it in
memory via `match_score` (`contacts.rs:341`); `idx_contacts_seen` is only used by the empty-query branch
(`:382`). `cleanup_candidates` (`:472-487`) is the same.
**Fix (a `LIKE` prefilter is wrong):** `match_score`/`score_field` is a subsequence/prefix scorer, so
`like '%q%'` would drop legitimate matches (query `abc` against contact `aXbXc`). Either build the pattern
from the query itself (`like '%a%b%c%'`, preserving subsequence semantics) or memoize the contact list
between keystrokes.

### A16 · `[removed]` — not a bug
`db/migrations.rs:470-476`'s `where path like '%&%-%'` + `decode_modified_utf7` is correct: before v12 every
stored path was raw server form, so decoding it is the right operation. The suggested guard
(`encode_modified_utf7(&decoded) == path`) holds for pre-v12 data too, so it would not prevent the rename it
was meant to prevent. No action.

### A17 · low · duplication and dead code
- `store/contacts.rs:380-396`, `:399-413`, `:472-487` — three copies of the same query + row mapping.
- `store/settings.rs:314-329` vs `:487-502` — `get_delay_secs` / `get_sync_interval` are one function twice.
- ~~`store/messages/attachments.rs:212-218` `delete_attachments_for_message` — **verified** no non-test callers.
  Dead per AGENTS.md; delete it and its test.~~ **Deleted**.
- `undo.rs:171-176` — `messages::get_by_uid` in a loop over a whole selection: N+1 over `get_by_uid`.

### A18 · low · column-order drift between `schema.sql` and an upgraded DB `[verify]`
`ALTER TABLE ADD COLUMN` always appends, so an upgraded DB puts `accounts.from_name`, `folders.server_total`,
`messages.from_name`, `attachments.data`, `contacts.alias`, `send_queue.raw_mime` at the end, not at their
`schema.sql` position. Verified no `select *` anywhere and every `row_to_*` names columns explicitly, so it is
harmless today — but `schema.sql` is not a faithful description of an upgraded DB.
**Fix:** record the caveat in the `migrations.rs` header, or rebuild the affected tables once to restore canonical order.

---

## B. `mailcore` sync & networking — `sync/`

### B1 · high · SMTP has no socket timeout, and blocks the async runtime — **FIXED** `[fixed]`
`sync/sender/client.rs:79-81,96-109,428` and `sync/sender/dsn.rs:40-46`

```rust
fn transport(&self, password: &str) -> Result<SmtpTransport> { self.transport_with_timeout(password, None) }
… let response = self.transport(password)?.send_raw(&envelope, raw)?;          // client.rs:428
let mut conn = SmtpConnection::connect((host, port), None, &hello, wrapper, None)?;  // dsn.rs:40
```
lettre 0.11 with `smtp-transport`/`pool` is the **blocking** transport; `send_raw` does connect+TLS+AUTH+DATA
inline. `.timeout(None)` is not "the default" — it disables both the connect timeout and the read/write
timeouts, so the doc comment "sends keep the transport default" is wrong. The `mailclient-net`
current-thread runtime then blocks forever on a half-open SMTP socket and every queued IMAP job stops until
restart. The DSN path has no timeout at all.
**Fix:** `.timeout(Some(secs(30)))` everywhere. `spawn_blocking` alone does **not** fix this — the net thread
runs queued jobs one at a time (`rt.block_on` per job), so the next job waits regardless of which thread the
SMTP call blocks. The timeout is the fix; moving off the runtime only reduces the blast radius.

### B2 · medium · `COMMAND_TIMEOUT` bounds each *read*, not the command `[corrected]`
`sync/imap/session.rs:122-128` (and `session/idle.rs:150-155`, `read_greeting`)

```rust
loop { let event = tokio::time::timeout(COMMAND_TIMEOUT, self.stream.next(&mut self.client)) …
```
Each iteration gets a fresh 30 s. A server that sends any untagged noise at <30 s intervals (`* OK Still
here`, a quota notice, or a hostile drip) keeps the loop alive indefinitely while the tagged reply never arrives.
**Fix (a single 30 s deadline is wrong):** one `Instant::now() + COMMAND_TIMEOUT` for the whole command would
fail large legitimate `FETCH BODY.PEEK[]` responses that take longer than 30 s to dribble in. Use a
command-scoped deadline derived from the command's expected size, or keep the per-read timeout and add a
separate, longer overall cap (e.g. `COMMAND_TIMEOUT` idle, plus an absolute bound per command kind).

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
full `BODY.PEEK[]` bodies in one command. See B19 for the transport-level cap and the corrected scope of this.

### B5 · low · unbounded net-job queue; one pool thread per send
`mailapp/src/bridge/worker.rs:143-172` (`mpsc::channel()` is unbounded; `spawn_flag_push`'s
`let _ = net_tx().send(..)` never coalesces) and `sync/sender/client.rs:403-434` — every `submit_raw` builds a
throwaway `SmtpTransport`, spawning a `lettre-connection-pool` thread that lives up to its 60 s `idle_timeout`.
A 50-row outbox flush spawns 50 threads.
**Fix:** build the transport once per flush; cap/coalesce the net queue.

### B6 · `[removed]` — same as D8
One sleeping OS thread per undoable action. Kept as **D8**, which covers both frontends
(`mailapp/src/bridge/worker.rs:135-141` and `crates/mailffi/src/net.rs:288-294`).

### B7 · medium · UIDVALIDITY change wipes the cache before the replacement exists — confirmed
`sync/imap/engine/sync.rs:77-81`

```rust
if validity_changed { log::warn!(…); messages::delete_by_folder(db, folder_id)?; }
```
Deletion happens at the top of `sync_folder_window`; every later step (flag fetch, backfill, Sent copy,
connection drop, task abort in `push.rs:181-187`) can leave the folder locally **empty** with the
badge/notification baseline lost until the next successful sync.
**Fix:** fetch the new window first (staging table) or defer the delete / write a "resync pending" marker.

### B8 · `[removed]` — the premise is false
`engine/sync.rs:81` (`delete_by_folder`) runs **before** `engine/sync.rs:141`
(`let local_uids = messages::list_uids(...)`), so after a validity change `local_uids` is empty and
step 3's `older_existing` (`:223`) is empty. The stale `folder.highest_modseq` is never used. No action.

### B9 · low · Trash `\Seen` sweep clobbers starred/draft locally — confirmed
`sync/imap/engine/sync.rs:55-59` with `store/messages/flags.rs:89-93`

```rust
let _ = messages::set_flags_by_uid(db, account_id, folder_id, *uid, true, false, false);
```
`set_flags_by_uid` writes **all three** columns, so a starred clean row in Trash loses its star and
`is_draft` is forced false until the next sync re-fetches.
**Fix:** a read-only setter, or pass the row's current starred/draft.

### B10 · low-medium · Trash sweep writes local state the server may not have — confirmed
`sync/imap/engine/sync.rs:49-60` — the STORE failure is only `log::warn`ed, yet local rows are marked read
unconditionally. A server rejecting the STORE (read-only mailbox, quota) leaves a permanent local/server
disagreement that nothing re-pushes, because the rows are not `flags_dirty`.
**Fix:** update locally only after a successful STORE, or mark the rows dirty.

### B11 · medium · two real bugs where the first draft put a phantom `[corrected]`
Withdrawn: the original claim (rows deleted after a failed move) is impossible, because
`session.uid_move(&clean, dest_path).await?` (`engine/mutate.rs:107`) returns before
`messages::delete_many_by_uids`. The two genuine defects nearby are:

**B11a** · `engine/mutate.rs:107` + the `uid_move` fallback — when `UID MOVE` is unavailable or fails noise,
the fallback path is `UID COPY` + `UID STORE \Deleted` + `UID EXPUNGE`. After a **timeout** between the
COPY and the EXPUNGE the messages can end up in both folders on the next sync, because the local delete
already ran and the server still holds a copy in the source.
**B11b** · `engine/mutate.rs:121` + `session.rs` `uid_expunge` — the per-UID `UID EXPUNGE` fallback, when
`UIDPLUS` is absent, degrades to a **mailbox-wide** `EXPUNGE`, deleting every other message carrying
`\Deleted` in that mailbox, including ones this client never touched.
**Fix:** for B11a, re-`SEARCH` the source after a fallback move before deleting locally; for B11b, only take
the mailbox-wide EXPUNGE when the client can confirm no other `\Deleted` messages exist, or leave the
EXPUNGE to the server/next session.

### B12 · `[removed]` — the ordering is deliberate
`engine/mutate.rs:99-106` marks `\Seen` **before** `uid_move` on purpose: after the move the UIDs belong to
the **destination** folder, so a subsequent `set_flags_by_uid` on the source folder would not find them.
The only residual effect of a failed move after a successful STORE is local-unread vs server-read, which the
next flag refresh corrects. No action.

### B13 · low · sessions checked back into the pool after an error — confirmed
`sync/pool.rs:89-92,113` vs `sync/attachments.rs:30-33` — the pool documents that anything but clean
completion should drop the session, but `imap.checkin()` also runs on the error path. It self-heals via
`is_healthy()`, but a timed-out session goes back with stale reply data pending.
**Fix:** `drop(imap)` / explicit discard on the error path.

### B14 · `[removed]` — no lock is held
`checkin()` (`pool.rs:101-112`) does `drop(pool)` **before** `s.disconnect()`, and the `Drop` impl
(`pool.rs:127-137`) never holds the mutex at all. `disconnect()` only clears the stored session. No action.

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

### B17 · low · send-path failures do not feed the backoff `[corrected]`
`sync/push.rs:373-382`

```rust
let report = background::push_check(&ctx.db, &ctx.db_path, account, imap).await;
if !report.skipped { ctx.listener.report(&report); }          // errors DO reach the UI
if !report.errors.is_empty() && !imap.is_healthy().await { return Err(…); }
```
Withdrawn: the claim "push swallows real errors" is false — `ctx.listener.report(&report)` forwards every
report, errors included, so they do reach the UI. The residual, narrower truth is that `report.errors` only
increments the account's failure counter via the NOOP branch, so a permanently failing outbox/SMTP never
enters the 30 s→15 min backoff for that account and is retried at full rate instead.
**Fix:** let `report.errors` count towards the witness failure tally.

### B18 · low-medium · sync lock is TOCTOU-racy and recursively unbounded — confirmed
`sync/headless.rs:498-527` — an empty or partially-written pid file parses to nothing → `is_none_or(...)`
yields "stale" → `remove_file` → retry. Two processes can each conclude the lock is stale and each hold it;
`acquire_sync_lock` also recurses without a depth bound (stack overflow if the path keeps coming back
"stale", e.g. a directory at that path).
**Fix:** `flock` / atomic create-and-verify, and a bounded loop instead of recursion.

### B19 · low · per-message attachment budget is only part-count × 25 MiB `[corrected]`
`sync/imap/parse.rs:150,157-159` + `sync/imap/engine/sync.rs:439-470` — `fetch_attachments` parses with
`with_bytes = true` and stores up to `MAX_ATTACHMENTS_PER_MESSAGE` (50) parts × `MAX_ATTACHMENT_BYTES`
(25 MiB) = 1.25 GiB into SQLite for a single message. Only *inline* images have a per-message budget
(`MAX_INLINE_BYTES_PER_MESSAGE`).
Scope correction: B4's "no byte budget on the response" is wrong at the transport level — imap-next 0.3.4
already caps each response (`imap-next/src/client.rs:53`, `max_response_size: 100 * 1024 * 1024`). So the
real exposure is 100 MiB resident per response before parsing, not unbounded.
**Fix:** a per-message byte budget for the download path (see C4), and rely on the imap-next cap for the
transport side.

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

### C1 · critical · `badge.rs:76` byte-index panic on a non-ASCII sender — **FIXED** `[fixed]`
`crates/mailcore/src/badge.rs:76`

```rust
let punycode = label.len() >= 4 && label[..4].eq_ignore_ascii_case("xn--");
```
`label.len() >= 4` does not guarantee that byte 4 is a char boundary. `domain_label`
(`badge.rs:93-110`) pops the TLD first, so a **single-label** domain with a multi-byte char is enough —
no dot needed. `[verify]` with the real `domain_label`:

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

**Fixed in `1dac9e7`.** `get(..4)` returns `None` when byte 4 is not a char boundary, so the encoding check
simply fails instead of panicking; the label then contributes its own first letter, which is the correct
answer for a real Unicode domain. Comment added at the site explaining why the length check is not enough.
Two tests added beside `a_punycode_domain_adds_no_letter`: `a_non_ascii_domain_label_does_not_panic`
(covers `abcö.com`, `x🎉.de`, `abc🎉`, `über.example.com`, `例え.jp` — note the third has no dot, which is
what makes `domain_label` keep the whole thing as the label) and `punycode_still_wins_over_a_multibyte_label`
(holds the ASCII and `xn--` answers unchanged, including the `example.com` control that needed the
second-level list to be skipped).

While writing them I got three expected values wrong before running (`AE` where the label's own first letter
was the answer), which is the point of asserting exact strings rather than just "does not panic" — the
`domain_label` TLD/second-level popping decides which label is tested, and that is easy to misread.

Verification: the panic was reproduced against the *old* line with the *new* tests in place
(`end byte index 4 is not a char boundary; it is inside 'ö'`), then re-run green afterwards, so the tests are
a genuine regression guard rather than a restatement of the fix. Gates per AGENTS.md §7:
`cargo fmt --check` OK, `cargo clippy -p mailcore -- -D warnings` OK, `cargo test -p mailcore` 564 passed
(562 before + 2 new). Full `cargo test --workspace` and the Qt build were not run — the change is inside a
`mailcore`-internal function with no API change.

### C2 · high · quadratic entity decode, applied twice per mail — **FIXED** `[fixed]` `[verify]`
`html/entities.rs:14`

```rust
if let Some(semi) = s[i..].find(';').filter(|n| *n < 24) {
```
`find(';')` scans to the **end of the string** before `< 24` is applied. Text nodes are split at `<`, so a
tag-free body is one node. `[verify]` with the exact algorithm:

| n | elapsed |
|---|---|
| 20 000 | 12.8 ms |
| 40 000 | 50.5 ms |
| 80 000 | 210 ms |
| 512 000 | **9.32 s** |

Input: a body of `"&".repeat(512_000)`. There is no body-size cap on ingest
(`sync/imap/parse.rs`, `mime.rs`), so a hostile sender can store a body that large.
**Correction that raises severity:** the decode runs **twice** per mail — `html::sanitize`
(`html/sanitize.rs:147`) and `html::html_to_text`, the latter on the **pre-sanitize** `candidate_html`
(`feed.rs:390`, also `sync/sender/message.rs:90-137`), neither capped. ~19 s of the net/GUI thread per
crafted mail; a mailing list of these starves every other job.
**Fix:** bound the scan window, not the result — `b[i+1..].iter().take(23).position(|&c| c == b';')`
finds the `;` without ever scanning past 23 bytes.

**Fixed in `946e971`.** Verified by measurement on the same 512 KB input: **11.49 s before, 12.4 ms after**
(~930×), and the decode runs twice per mail, so the crafted-mail cost was ~23 s of the net thread.
Two tests: the ampersand flood itself (`"&".repeat(512 * 1024)` round-trips unchanged), and a boundary pair
that pins the window at 24 bytes — a 24-byte entity decodes, a 25-byte one stays literal — so widening the
window later fails instead of quietly reintroducing the cost.

### C3 · medium · CSS clickjacking: invisible full-body link overlay survives sanitizing `[corrected]`
`html/css.rs:70-73` allows `display`, `width`/`height`, `margin`, `opacity`; `allowed_display` permits
`block`; `safe_value("0")` passes. `presentational(tag, "style", v)` (`sanitize.rs:81`) applies it to **any**
allowed tag, including `a`.

```html
<a href="https://evil.example.net/" style="display:block;width:100%;height:1400px;opacity:0;margin:-16px 0">…</a>
```
The reader document CSS (`html/reader.rs:293-307`) only sets `a{color}` — no `pointer-events` guard — so the
transparent rectangle covers the mail and steals the click. The CSP keeps `style-src 'unsafe-inline'`, so
nothing blocks it downstream.
Severity trimmed from medium-high: the user must click inside the overlay, and the click is at a chosen
point rather than a guaranteed one. Still in-page phishing with no script and no permission prompt.
**Fix:** clamp `opacity` to a visible minimum (or drop the declaration below ~0.15), and refuse
`display:block` + size on `a`.

### C4 · medium · no parse-side size cap: a 25 MB `.ics`/`.vcf`/DSN is parsed on every open — confirmed
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

### C5 · medium · unbounded DSN recipient expansion — confirmed
`report.rs:152` — `parse_dsn` has no recipient cap (unlike `vcard.rs:15`'s `MAX_ENTRIES`). Every block with a
`Final-Recipient` becomes a 7-field struct serialized into `message_json`. A 20 MB `message/delivery-status`
part of ~600 000 `Final-Recipient:` blocks yields several hundred MB of `ReportRecipient`s plus a multi-MB
JSON payload, on every message open.
**Fix:** cap `dsn.recipients` (e.g. 50) as the vCard parser does.

### C6 · medium · an unclosed drop-content tag swallows the rest of the message `[corrected]`
`html/sanitize.rs:44-58` — nothing but a matching close tag ever lowers `drop_depth`; not `</html>`, not
`</body>`, not EOF. `drop_content_tag` includes `style`, `head`, `template`, `form`, `title`, `noscript`
(`html/tags.rs:100-117`). `if drop_depth == 0` at `sanitize.rs:146` gates every text run.
**Correction that raises the impact:** the most realistic trigger is a real mail whose `<head>` is never
closed — which most mail has — so the whole body disappears, not just an optional trailer. Also reachable
via `<p>visible</p><template>`, or any `</style>` mangled in transit.
**Fix:** reset `drop_depth` to 0 at EOF, and/or treat `</html>`/`</body>` and a second document-level
`<style>` as closing.

### C7 · medium-high · `is_public_remote` accepts every bracketed IPv6 host and the metadata IP — **FIXED** `[fixed]`
`html/urls.rs:42-63`

```rust
let host = host.split(':').next().unwrap_or("");     // <-- mangles IPv6 to "["
if host.is_empty() || host == "localhost" || host.starts_with("127.")
    || host == "[::1]" || host == "::1" || host.starts_with("10.") || host.starts_with("192.168.") …
```
The `.split(':').next()` on line 43 (added to strip a port) reduces any bracketed IPv6 literal to `"["`,
which passes every check — so **every** IPv6 URL, `[::1]` included, is treated as public. Independently,
`169.254.0.0/16` (incl. cloud metadata `169.254.169.254`), `100.64.0.0/10`, `fc00::/7`, `fe80::/10`,
`0.0.0.0` and `inet_aton` forms (`2130706433`, `127.1`) are all missing. An
`<img src="http://169.254.169.254/latest/meta-data/…">` is fetched whenever the user enables remote images.
The response is not readable by the page, so this is internal port scanning / metadata probing, not
exfiltration.

**Fixed in `11137f4`.** `IpAddr::is_global` was not used (it is `#[unstable(feature = "ip")]`); the ranges
are written out instead:
- new `http_host()` returns the authority host with user info, the `[..]` wrapper and a real port removed.
  A bare IPv6 literal (more than one `:`, no brackets) is left whole rather than cut at the first colon,
  and a bracketed literal is unwrapped. A zoned `[fe80::1%25eth0]` is unwrapped too.
- `is_public_ipv4` blocks `0/8`, `10/8`, `127/8`, `100.64/10`, `169.254/16`, `172.16/12`, `192.0.0/24`,
  `192.168/16`, `198.18/15` and `240/4` (reserved + broadcast), by octet masks.
- `is_public_ipv6` unwraps the IPv4-mapped (`::ffff:169.254.169.254`) and IPv4-compatible (`::10.0.0.1`)
  forms into the v4 rule first, then blocks `::`, `::1`, `fc00::/7`, `fe80::/10` and multicast.
- `loose_ipv4` catches the `inet_aton` shorthands a browser resolves (`127.1`, `10.1`,
  `2130706433`, and `2852039166` — the numeric twin of the metadata address) so blocking a range cannot
  be sidestepped by a number. Digits and dots only, so a hostname is never mistaken for a number.
- **Two gaps found while writing the tests:** a trailing root dot defeated both `ends_with(".local")` and
  the address parse, so `http://foo.local./` and `http://127.0.0.1./` were allowed; the host is now
  `strip_suffix('.')` once before any comparison.

Existing behaviour deliberately kept: `localhost.example.com` is still blocked by the pre-existing
`contains("localhost")` name check, and the numeric forms `127.1` / `127.0.0.1.` were *already* blocked by
the old `starts_with("127.")` prefix — they are guards against a regression, not newly-closed holes.

Six tests added in `html/tests.rs`, including `blocked_hosts_never_reach_the_reader_html`, which drives the
same hosts through `sanitize(…, true)` and asserts the host string never reaches the output HTML while
`had_remote` still records them (so the reader still offers the "show remote images" affordance). The old
host parsing was reproduced against the new lists in a scratch program: `[::1]` → host `"["` not blocked,
`[fc00::1]` → `"[fc00"` not blocked, `[::ffff:169.254.169.254]` → `"["` not blocked, `169.254.169.254`,
`100.64.0.1`, `2130706433` and `foo.local.` all not blocked.

Gates per AGENTS.md §7: `cargo fmt --check` OK, `cargo clippy -p mailcore -- -D warnings` OK,
`cargo test -p mailcore` 570 passed (564 before + 6 new).

### C8 · low · a bogus comment/PI with no closing token swallows the rest — **FIXED** `[fixed]`
`html/tags.rs:132-139` — for `<!-->` the search starts *past* the closing `>`, finds nothing, and consumes
the rest of the document; `<!--->` likewise. Input `<!--><p>everything after this is gone</p>`.
**Fix:** if `bytes[i+3] == b'>'` close the comment at `i+4`; likewise for `<!--->`.

### C9 · low · a PI with no `?>` swallows the rest — **FIXED** `[fixed]`
`html/tags.rs:145-150` — returns `(None, bytes.len())` when `find_sub(bytes, b"?>", i)` finds nothing. HTML
ends a bogus comment at the first `>`. Input `<p>ok</p><?x ><p>rest</p>` → `rest` never appears.
**Fix:** end at the first `>` as HTML does, and only treat `<?xml … ?>` as a PI.
*(Done as "always the first `>`": that already ends `<?xml … ?>` correctly, and a `?>` search first would let
an unterminated `<?` stretch to an unrelated `?>` further down.)*

### C10 · low · inline-image expansion escapes `MAX_OUT_BYTES` — confirmed
`html/inline.rs:120-160` — `MAX_INLINE_BYTES_PER_MESSAGE` is 6 MB of raw bytes, and each `cid:` hit appends
`base64_encode(&img.data)` (~4/3 → ~8 MB) directly to `out` with no `MAX_OUT_BYTES` re-check (unlike
`sanitize`'s `push_capped`). Four 1.5 MB inline PNGs referenced from `<img src="cid:…">` produce an ~8 MB
document that `reader::document` embeds.
**Fix:** count the produced length against `MAX_OUT_BYTES` (or a document budget) and stop substituting.

### C11 · low · `is_body_referenced` re-lowercases the whole body once per attachment — confirmed
`feed.rs:691-695` — `is_body_referenced` → `img_cid_references(html)` → `html.to_ascii_lowercase()`
(`html/inline.rs:66`), a full copy + scan of the body **per attachment** (up to 50). 512 KB × 50 ≈ 25 MB of
copy+scan per message open. Much worse now that C2 is known to apply to the same path.
**Fix:** compute `let refs = html::img_cid_references(body_html)` once outside the filter and test `refs.contains(...)`.

### C12 · low · unbounded Tier-2 similarity scan, no SQL `LIMIT` — confirmed
`similar.rs:263-275` — `let mut rows = stmt.query(params![account_id, target_from])?;` with no limit, and
every row's subject normalized in Rust. The loop only breaks once enough *matching* ids are collected, so a
sender with a large history and no matches walks the whole set on the feed thread.
**Fix:** add `limit ?N` (a small multiple of `remaining`) or stream with an explicit cursor.

### C13 · `[removed]` — documented decision, not a dead branch
`mime.rs:288-311` — the identical `return None` arms look dead, but the comment above them states the rule:
"A ZIP-subtype sniff (docx/xlsx/…) is more specific than a plain ZIP or generic header, but never overrules a
different specific type." Collapsing or changing it would reverse that decision. If anything, add a comment
noting the two arms are intentionally identical. No action.

### C14 · low · fragile fixed-width slice in vcard — **FIXED** `[fixed]`
`vcard.rs:369` — `s[..1].make_ascii_uppercase();` is safe only because `s` is always one of
`"mobile" | "fax" | "pager"` today. Same class as C1.
**Fix (the one-liner form fails clippy):** `Option::map` with `str::make_ascii_uppercase` (which takes
`&mut self`) trips `clippy::explicit_auto_deref`/borrow errors under `-D warnings`. Use
`if let Some(c) = s.get_mut(..1) { c.make_ascii_uppercase(); }`.

### C15 · medium · calendar component stack — **FIXED, reason corrected** `[fixed]`
`calendar.rs:157-211` — `stack.push(comp)` runs per `BEGIN:` with no cap, and the matching `END` does
`stack.iter().rposition(|c| *c == comp)` (`:200`), a full scan per `END`. A 25 MB `.ics` of
nested `BEGIN:X`/`END:X` is therefore **O(n²)** on top of the memory blowup, so a 25 MB file effectively
hangs rather than merely allocating. This is the amplifier behind C4 and the reason C4 is not just a
memory issue. *(Superseded — see the correction below: it is linear, so C4 stays a memory/CPU issue.)*
**Fix:** cap `stack.len()` (e.g. 64) and bail out past it.

**Fixed in `9698314`** — depth capped at 32, deeper input refused rather than parsed.

**The stated reason is wrong and was corrected.** The first draft called the `rposition` scan quadratic;
the second reviewer agreed and called it a hang. Both wrong. `rposition` scans from the *innermost*
component backwards, and the innermost almost always matches, so each `END` is O(1) in practice — the
worst case is bounded by the number of *distinct* component names in the stack, ~14 in iCalendar, not the
depth. Measured with the real parser on nested input:
**linear** (2 000 → 6.2 ms, 16 000 → 62 ms, 128 000 → 542 ms, 1 048 576 → **4.10 s**), so ~8× depth costs
~8× time. That is a real 4 seconds of feed thread per 13 MB attachment, and unbounded memory on the
component stack — a genuine CPU/memory bound, but not a quadratic blowup and not a hang.
The code comment now records the measurement and names the wrong first guess, and the severity should be
read as medium for that reason alone.

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

### D1 · high · net-thread bootstrap `expect`s, and a latched `busy` — **FIXED** `[fixed]`
`crates/mailapp/src/bridge/worker.rs` (net-thread bootstrap)

```rust
let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime for mailclient-net");
… .spawn(move || { … }).expect("mailclient-net thread");
```
`net_tx()` is lazily initialised from the first `spawn_job`/`spawn_flag_push`, i.e. from a QML click on the
GUI thread; both `expect`s abort the process there, and every later job silently no-ops (the
`let _ = net_tx().send(...)` swallows it).
**Correction that raises the severity:** the net thread itself cannot die — `worker.rs:163-167` wraps every
job in `guard("background job", …)`, so a panic inside a job is caught and the `while let Ok(job) = rx.recv()`
loop continues. The real second-order failure is `busy`: `spawn_job` sets it (`worker.rs:191`) and only
clears it inside the `qt.queue` completion callback (`worker.rs:227`). If that callback never runs —
`shutdown`, or the `queued` `Err` branch at `worker.rs:235` that the code itself flags with a comment — `busy`
stays latched and **every later job is refused** with the busy message until restart.
**Fix:** initialise the runtime eagerly at startup and return an error to QML instead of `expect`; clear
`busy` in the shutdown/failure path at `worker.rs:235`.

**Fixed in `74d3a4d`**, differently from the fix the first draft proposed:
- The runtime build moved **inside** the spawned thread rather than staying on the caller's thread.
  "Initialise eagerly at startup" was wrong for the abort itself: `main.rs` cannot build a runtime
  before `main`, and pre-building a runtime just to hand it to a thread is not possible in tokio, so the
  only way to keep the build off the GUI thread was to let the thread do it. `net_tx()` is now `net()`,
  returning `Result<&Sender, &'static str>` out of a `Net` enum (`Ready` / `Failed`).
- **`busy` is latched only after a job is actually queued.** The new `entry_gate(busy, net)`
  (`worker.rs:230`) holds the whole decision and hands back either the queue or the message to show.
  This is the real fix for the wedge: `Failed` now refuses *before* `set_busy(true)`, so a job that can
  never run can no longer leave the latch stuck.
- A dead net thread is reported as `network is unavailable: cannot start the net thread: …`, never as
  `BUSY_MESSAGE`. The two need different reactions ("restart" vs "try in a moment") and conflating them
  was half the bug. Busy still outranks a missing thread, because from the user's chair that is the more
  likely read.
- Sticky on failure, documented in `net`: both causes (OS refusing a thread, runtime refusing to build)
  are permanent for the process, and retrying on every click would spawn a thread each time in the
  pathological case.
- The `queued` `Err` branch at the end of `spawn_job` is unchanged and still only logs: with a live
  thread that owns its receiver the send cannot fail, so the only way to reach it is the QObject being
  gone at shutdown, which is what its comment already says.

### D2 · medium · unchecked narrowing on a destructive bulk path — confirmed
`crates/mailapp/src/bridge/messages/bulk.rs` (uid guard is in `crates/mailapp/src/bridge/messages.rs:40`)

```rust
let uid = x.as_u64().ok_or_else(|| "invalid selection".to_string())? as u32;
```
`parse_hits_json` guards the same cast (`bridge/messages.rs:76-80`: `if folder.is_empty() || uid == 0 || uid > u64::from(u32::MAX)`).
A payload uid above `u32::MAX` wraps and `mark_read_many`/`set_star_many`/`delete_many`/`archive_many`/
`move_many`/`purge_many` act on a *different* message id. Today the feed cannot produce one — it is a missing
guard on a destructive path.
**Fix:** mirror the `parse_hits_json` guard.

### D3 · medium · every message API crosses the bridge as `uid: i32` while `mailcore` UIDs are `u32` *(suspected)*
`bridge.rs:221,226,279,285,291,296,302,312,317,322,415,421,426,433,491,496,506,514,520,525,537,647,653` vs
`mailcore/src/models.rs:134`. A UID ≥ 2³¹ arrives negative and every method bails out
(`bridge/messages.rs:104-107`), so such mail cannot be opened, starred, deleted or exported from Qt at all.
`find_similar_json` is worse: `uid as i64` (`bridge/messages.rs:221`) sign-extends the negative value into
the SQL `uid = ?` comparison, so it silently matches nothing. Real servers exceed 2³¹ on large mailboxes —
hence "suspected" rather than confirmed.
**Fix:** widen to `i64`/`u32` for uid and add a range guard.

### D4 · low · uid/attachment ids kept in QML `int` — confirmed
`EmlExportDialog.qml:13`, `MessageView.qml:93`, `Main.qml:61` use 32-bit signed properties/signals.
`MessageView.showRemoteOnce` compares a `double` (`root.message.uid`, MessageView.qml:130) against an `int`
(`messageUid`), which disagree above 2³¹. Attachment ids are the same shape: feed/`i64`
(`messages::get_attachment(db, attachment_id as i64)`, `bridge/messages.rs:386`) squeezed through
`attachment_id: i32` (`bridge.rs:415,421`). Low because SQLite rowids that high are implausible.
Verified-correct for contrast: `bridge.rs:877-878` (`.min(i32::MAX as u64) as i32`), `bridge/messages.rs:250,116,135`.

### D5 · high · whole-cache JSON re-serialised on the GUI thread per interaction
`crates/mailapp/src/bridge/worker.rs` verifies the *net* rule sound end to end: all IMAP/SMTP runs on the one
`mailclient-net` current-thread runtime (`worker.rs:143-172`), every job is queued there
(`bridge/sync.rs:29,96,128,171,202,281`; `composer.rs:40`; `capabilities.rs:12`), and all three signals are
delivered through `qt.queue(...)` (`worker.rs:75,213,231`). No off-thread signal emission and no dialing-out
on the GUI thread exist.

The *DB/IO* half does not hold. `shared_db()` (`bridge.rs:768-795`) is a leaked per-thread
`rusqlite::Connection` used from ~40 `#[qml_element]` methods. Documented as deliberate, but silent where it
hurts: `push_feeds` (`bridge.rs:861-910`) re-serialises the **entire folder cache** to JSON and assigns it to
a `QString` property on the GUI thread — on every `open_message`, `toggle_star`, `mark_read`,
`select_folder`, `set_sort`, and after every finished job (`worker.rs:223-225`), with the limit set to the
full cached count (`bridge.rs:873`, `feed::messages_list_json_paged(db, folder_id, cached, 0)`).
Also per keystroke: `MessageList.qml:381` calls
`list_filter_keep(JSON.stringify(filter), JSON.stringify(rows))` — the whole feed stringified into Rust and
an index array back — plus `search_json`/`contacts_json` (`bridge.rs:140,176`).
**Fix:** paged/partial feed pushes; debounce keystroke-driven work onto the net thread.

*(D17 from the first draft — "`messages_json` always carries the full local cache" — was the same finding
seen from `bridge.rs:873`; it is merged here rather than counted twice.)*

### D6 · high · file IO and OS IPC on the GUI thread — **PARTLY FIXED** `[fixed]`
- **`bridge/composer.rs:81-86` → `sync::sender/inline.rs:96-114`:** `compose::image_data_url` does
  `let bytes = std::fs::read(&path)` **first** and only then `if bytes.len() > MAX_INLINE_IMAGE_BYTES`
  (1.5 MB). So the *whole* file is read and buffered on the GUI thread from `Composer.qml:416` inside the
  drop handler, before any size check, and the resulting `data:` URL is then pushed across the bridge.
- `bridge/composer.rs:125,144` → `stage_forward_files`/`stage_resend_files`/`stage_draft_files`
  (`compose/forward.rs:97`, `compose/drafts.rs:191`) write attachment copies to temp dirs **and** run
  `prune_stale_draft_dirs` (`paths.rs:268-293`, a `read_dir` walk plus `remove_dir_all`) — all on the GUI thread.
- `maintenance.rs:41-46` `cleanup_temp` → `remove_dir_all` on the GUI thread; `maintenance.rs:21-29` stats the
  DB file and walks the temp dir on the GUI thread (Settings.qml:1040-1045 admits it).
- `bridge/accounts.rs:56` (`add_account` → `account_form::save` → `auth::save_account_secrets` →
  `keyring::Entry`) and `accounts.rs:119` do Secret Service D-Bus round trips on the GUI thread; a hung
  gnome-keyring/kwallet freezes the window.
**Fix:** size-check with metadata (`fs::metadata`) before reading; move all four behind `net_tx()` or a
dedicated IO job.

**Fixed in `c3d90fb`, first bullet only.** `image_data_url` now stats the file first and reads with a
`take(limit + 1)` cap, so an oversized file is refused without being loaded and a file that grows between
the two cannot overrun. The error reports the **real** size rather than the truncated limit, which is what
the user wants to know. The oversized test uses a sparse file so a 4 GiB input stays cheap to create.

**The other three bullets are still open, and not for lack of trying.** Each one is a QML-callable method
that returns its result synchronously — `add_account` returns a status string and the composer needs the
staged paths to continue — so moving any of them off the GUI thread needs an async contract (queue it, then
deliver a signal and let QML advance). That is an API change in both frontends, not a local fix, and it is a
decision to take rather than an item to close: the keyring round trip is D-Bus on Linux (a file on Android),
the staging walks the cache dir on every draft open and forward, and the maintenance calls are user-triggered
from Settings. Leaving them on the GUI thread means a hung gnome-keyring can still freeze the window.

### D7 · medium · a SQLite read inside a property *binding* — confirmed, severity trimmed
`Main.qml:1004-1005` — `autoSyncMinutes: … appSettings.sync_interval_for(backend.current_account_id)` →
`account_settings::sync_interval` (`store/settings.rs:38-42`). Re-evaluated on every account switch and every
`syncSettingsRevision++` (Settings.qml:1866). Severity is medium only in that it is a small indexed read.
**Fix:** cache the value in `SettingsBridge` behind the existing revision counter.

### D8 · medium · a new OS thread per undoable action
`bridge/worker.rs:135-141` `spawn_push_after_grace` and `crates/mailffi/src/net.rs:288-294`
`spawn_push_after_grace` each `std::thread::spawn` a sleeper per undoable action
(`api/mutate.rs:106,194` calls it). N deletes in one session = N sleeping threads plus N queued IMAP pushes,
unbounded by any cap. *(This replaces first-draft B6.)*
**Fix:** coalesce into one sleeping task keyed on the existing in-flight table (E10).

### D9 · low · timer polling does a DB read
`Main.qml:993-999` `pendingOpenTimer` (2 s repeat) calls `consume_pending_open()` → a SQLite settings read on
the GUI thread. Documented as cheap.

### D10 · low · two `ScrollView`s whose content does not bind width to the ScrollView's own id `[corrected]`
`Main.qml:1920-1926` and `Composer.qml:772-781`

```qml
ScrollView { Layout.fillWidth: true; Layout.fillHeight: true; clip: true
    TextArea { id: statusTextArea; text: root.statusText; wrapMode: TextArea.WrapAnywhere; … } }
```
**Correction:** a lone `TextArea` inside a `ScrollView` is the standard Qt arrangement, and a `TextArea` with
`wrapMode` set is not the `parent.availableWidth` antipattern the AGENTS.md rule targets (that rule is about a
`Column`/`Item` that falls back to its implicit width, disabling wrapping and pushing trailing controls
off-screen). Every other ScrollView in the app does bind correctly
(Settings.qml:638,686,741,798,897,1049,1145; MessageView.qml:1326; AccountSetup.qml:219), which is why these two
look inconsistent — but I could not confirm a wrapping defect without running the GUI, so this is **not**
rated high and is out of the fix order until checked on a narrow window. The one hard claim worth keeping:
`statusTextArea` carries a status line whose length is unbounded, and if it *does* wrap at the wrong width a
long error sentence cannot be read.
**Fix:** if it reproduces, bind each child's `width` to the ScrollView's own `id`.

### D11 · medium · core protocol parsing in JS `[corrected]` — confirmed, severity trimmed
- `Main.qml:1456-1464` `undoMove(batch)` re-implements how undo batches are encoded — `batch.split(",")`,
  while Rust joins them in `bridge/messages/bulk.rs:193-197`.
- `Main.qml:777` `parseInt(nl < 0 ? r : r.slice(0, nl), 10)` decodes the `"<id>\n<folder>"` pending-open
  payload in JS.
- `MessageView.qml:170-181` `joinFileUrl` rebuilds a percent-encoded `file://` URL Rust already has
  (`bridge/messages/files.rs:8-24`).
- `MessageView.qml:376-384` `baseName` re-implements filename decoding.

Correction: the fix-order draft referenced `MessagesView.qml:170-181` and `:376-384`; that file does not
exist — both helpers live in `MessageView.qml` as cited above. Severity is medium because it is duplicated
parsing logic, not a wrong result.
**Fix:** expose the parsing from Rust and have QML consume the result.

### D12 · low · a JNI round trip inside a property binding, plus dead change handlers `[corrected]`
`MessageView.qml:79-81,115`

```qml
readonly property int fitBelow: root.isHtml && root.backend ? root.backend.reader_fit_below(root.shownHtml) : 0
readonly property bool fitLayout: !root.originalColors && root.fitBelow > 0 && bodyLoader.width > 0 && bodyLoader.width < root.fitBelow
…
onFitLayoutChanged: root.reloadHtml()
```
**Correction:** the first draft claimed every reader resize rebuilds a Chromium page. It does not —
`fitLayout` is a **bool** with a width threshold, so `onFitLayoutChanged` fires only when `bodyLoader.width`
crosses `fitBelow`, not per pixel. The suggested fix was already what the code does.
What remains: `fitBelow` calls `backend.reader_fit_below(root.shownHtml)` — a full JNI round trip over the
whole document — from inside a **property binding**, re-evaluated whenever `shownHtml` changes; and
`reloadHtml` also runs on `onRemoteHtmlChanged`/`onLoadRemoteImagesChanged`/`onAllowRemoteOnceChanged`
(`MessageView.qml:111-113`), which can fire transiently and rebuild the page.
**Fix:** compute `fitBelow` once per message into a plain property (or cache it in `reader_document`,
`bridge/messages.rs:253-283`, which already re-reads `headerBlock.height` and re-quotes the theme palette),
and guard the reloads against no-op changes.

### D13 · low · the list mutates a feed it does not own — confirmed
`MessageList.qml:454-457` — `hits[i].key = hits[i].folder_id + ":" + hits[i].uid;` writes into `Main.qml`'s
`searchRows` objects while rebuilding. `MessageList.qml:832` `onContentYChanged: root.rememberScroll()`
calls `indexAt`/`itemAtIndex` on every scroll-pixel change during a flick.
**Fix:** build a local proxy list; throttle scroll memory.

### D14 · medium · non-resizable dialogs, against the stated rule — confirmed, severity trimmed
Only the large managers use `AppDialog`; every small aux dialog is a plain `Dialog` with a fixed `width:`
(and often a fixed `height:`) and no resize grip: `Main.qml:1724-1730` (`deleteConfirm`),
`Main.qml:1793-1799` (`purgeConfirm`), `Main.qml:1871-1877` (`statusDetailsDialog`, also
`height: Math.min(380, root.height - 64)`), `MessageView.qml:1125-1130` (`examineLinkDialog`),
`MessageView.qml:1288-1294` (`headersDialog`, fixed width **and** height), `Composer.qml:938-944,989-995,1050-1056`,
`Accounts.qml:177-183`, `Settings.qml:1267-1272`, `components/ImagePlacementDialog.qml:10-26`. The
status-details dialog is the practical loss: a long error sentence cannot be enlarged.
**Fix:** migrate to `AppDialog` with geometry memory.

### D15 · low · main window width expression has no floor — confirmed
`Main.qml:20-21` `width: Math.min(1320, Screen.desktopAvailableWidth - 80)` goes negative on a screen
narrower than 80 logical px (clamped by `minimumWidth: 380`, but the initial geometry is nonsense).
Otherwise the responsiveness rules are followed: wrapping labels carry `wrapMode` + bound width, `RowLayout`
children that must yield carry `Layout.minimumWidth: 0` (BulkActionBar.qml:37, Accounts.qml:98-135,
Folders.qml:156, Outbox.qml:127-169, MessageView.qml:830, Settings.qml:1189, AccountSetup.qml:293),
`Flow`s are `Layout.fillWidth` (Settings.qml:1108,1230; ComposerAttachmentTray.qml:40).

### D16 · medium-high · the reader payload copies each body three times — confirmed, severity trimmed
`mailcore/src/feed.rs:562-565`

```rust
"attachments": files, "body_text": plain, "body_html": body_html,
… "body": legacy_body,     // legacy_body is a clone of whichever of the two was chosen
```
A large HTML mail is serialised 3× into one JSON string, copied into a QML JS object (`Main.qml:198`), held in
`currentMessage` while `MessageView` also builds a wrapped document — and `Main.reloadMessages()` re-fetches
the whole payload after every job finish, star toggle, bulk action and sort change. On top of that,
`MessageView.showRemoteOnce` (MessageView.qml:130) pulls the same body again through the separate
`message_html` route (bridge.rs:216-220 documents why). Real memory cost scales with mail size, so medium-high
rather than critical.
**Fix:** drop `legacy_body`, paged the feed, and cache the reader document per message.

### D17 · `[removed]` — merged into D5
Same as `bridge.rs:873`; see the note at the end of D5.

### D18 · low · one `WebEngineView` kept alive for the app's lifetime — confirmed
`Composer.qml` is a `Dialog` parented to `Overlay.overlay` holding `EditorFrame`'s `WebEngineView`
(EditorFrame.qml:118); closing the dialog hides it but does not release the page, and its 200 ms
`document.queryCommandState` poll (EditorFrame.qml:158-163) keeps running whenever the dialog is
invisible-but-`ready`.
**Fix:** destroy the WebEngineView on close (or stop the poll while hidden).

### D19 · low · a deliberately leaked connection per bridge thread — confirmed
`bridge.rs:791` `Box::leak`s a `rusqlite::Connection` per thread that touches the bridge (documented, bounded
at GUI + net today). The `mpsc` channel in `net_tx()` keeps job closures alive until the net thread drains
them; nothing caps that queue, so a burst of dropped requests retains all closures until thread exit.

**Verified clean:** no QML reference cycles (`FeedJson`/`ModelSync`/`AccountOverrides` are stateless
singletons; `Sidebar.expandedById`/`MessageList.scrollMemory`/`headersInfo`/`currentMessage` are plain JS
objects; `backend` references point at objects owned by `Main.qml`). No binding loops (header/spacer,
`fitLayout`, filter and scroll-memory paths all resolve without re-entering a binding). `Connections`
targets and handler names are right (`onJob_finished`/`onJob_progress`/`onUndo_available`). Timers are
children of their owner. Delegate models use `required property`. `undo_available`/`job_finished`/
`job_progress` are emitted only from `qt.queue` closures. One nuance: `bridge/messages/bulk.rs:194-195,246-247`
emits `undo_available` synchronously on the GUI thread inside the invokable, safe only because the row-level
emits elsewhere are deferred with `Qt.callLater`.

---

## E. Native Android + FFI — `crates/mailffi/src/`, `android/`

### E1 · medium · `auth_vault.json` is not excluded from device-to-device transfer — **FIXED** `[fixed]`
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
device. Note this file is the one referenced by `AndroidManifest.xml:39`
(`android:dataExtractionRules`) and governs **both** cloud backup and device transfer on API 31+.
Severity lowered from critical: device-to-device transfer goes to the user's **own** device, so it requires
the old device to be in the attacker's hands or on the same account.

**Fixed in `d3ef68b`**, by the same commit as E2 — the new `<device-transfer>` section excludes
`auth_vault.json` (and the cache), so the secrets file no longer transfers either. The two items shared one
section; fixing one without the other would have left the hole open, which is why they were landed together.

### E2 · high · the whole local mailbox is in cloud backup — **FIXED** `[fixed]`
`android/app/src/main/res/xml/backup_rules.xml` and `data_extraction_rules.xml`, referenced from
`AndroidManifest.xml:38-39`

```xml
<full-backup-content>
    <exclude domain="file" path="auth_vault.json" />
</full-backup-content>
```
`fullBackupContent` with no `<include>` means "everything except these", so `mailclient.sqlite` (cached
subjects, senders, snippets, and full bodies/attachments once read) and `filesDir/crashes/*.log` are uploaded
despite the comment claiming "Mail itself stays on the server".

**Fixed in `d3ef68b`.** Both files now exclude `auth_vault.json`, `mailclient.sqlite`, `mailclient.sqlite-wal`,
`mailclient.sqlite-shm` and `crashes/`, and `data_extraction_rules.xml` gained a `<device-transfer>` section
carrying the same set (E1's list) so the API 31+ path cannot leak through it. Details that the first draft
missed and that the fix now covers:

- **WAL mode matters.** `db/mod.rs:61` sets `pragma journal_mode = WAL`, so un-checkpointed pages live in
  `mailclient.sqlite-wal`/`-shm`. Excluding only the `.sqlite` would still upload the bulk of the cache.
- **`domain="file"`, not `"database"`.** The DB is opened at an explicit path under `filesDir`
  (`MailNative.kt:22` → `mailcore::use_data_dir`), never via `getDatabasePath()`, so SQLite does not
  register it in the `database` domain. A future reader switching the domain back would silently un-exclude it.
- **What backup still preserves:** the three scheduling preference files
  (`mailclient_alarm`/`mailclient_worker`/`mailclient_push`, `shared_pref` domain), so background checks come
  back configured after a restore. Accounts live in the excluded DB and are re-entered — which is also why
  excluding it is the *correct* behaviour rather than only the private one: a DB restored without its vault key
  could not decrypt anything. Draft staging dirs are in `cacheDir`, which auto backup never covered.

**Verification:** XML well-formed (`xmllint`/`ET.parse`) and both files compiled with
`aapt2 compile --dir res` (exit 0), with the flat files dumped to confirm every exclude is present in both
`<cloud-backup>` and `<device-transfer>`. The full `./build.sh --android` was **not** run (skipped on request
as too slow); a resource-only change cannot affect compiled code, and aapt2 is the tool that validates it.

**Follow-up decision, not a defect:** with the cache and secrets excluded, backup no longer carries the
account list. If that restore path is not wanted either, `android:allowBackup="false"` is the equivalent and
is safer by default for any *future* file added to `filesDir` — an exclude list protects only the paths listed
today. Left as-is for now because the project's own comment states the intent as "mail stays on the server",
which is what this now enforces.

### E3 · medium · `MailNative.init()` opens SQLite and migrates on the UI thread — confirmed
`ui/shell/MailShell.kt:291-294` → `ensureInit` → `init()` → `use_data_dir()` + `shared_db()` →
`mailcore::Db::open` = "Open (creating parent dirs) and migrate to the current schema"
(`mailcore/src/db/mod.rs:35-43`). When the UI process is the first into the library, a post-update migration
of a large cache runs on the main thread.
**Fix:** call `ensureInit` from `MailApplication.onCreate` on a background thread (also fixes E19).

### E4 · low · `spawn` → `forward_busy` invokes Java from the caller's thread — confirmed
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

### E5 · low · attach policy is weaker than assumed `[corrected]`
`crates/mailffi/src/android.rs:1346` and `:247` — `self.vm.attach_current_thread(|env| …)`.
**Correction:** jni 0.22's `attach_current_thread` (`jni-0.22.4/src/vm/java_vm.rs:488`) "requests to
permanently attach the current thread", is cheap when already attached (a TLS check, no JNI call), and
guarantees attachment for the callback's duration; `attach_current_thread_for_scope` (`:532`) is the scoped
variant that detaches. So there is **no** per-event attach/detach cost, and no fix is needed for the cost
itself.
What remains is accuracy and safety: the call still allocates a `DEFAULT_LOCAL_FRAME_CAPACITY` local frame
on each invocation, and the "permanently attached" semantics are pre-existing-attachment-dependent (the
docs warn a scoped attachment higher on the stack takes precedence). The net thread should therefore hold a
single long-lived `AttachGuard` (`attach_current_thread_guard`) rather than re-entering per event, and a
future switch to `_for_scope` — which a reader might reach for by name — would reintroduce the per-event
attach/detach this doc previously warned about.
**Fix:** hold one `AttachGuard` for the net/monitor thread; add a comment pinning the choice.

### E6 · low · `uid < 0` is silently clamped to UID 0 instead of rejected — confirmed
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
silently flips the UI to "delete permanently, always confirm" with no clue why. Severity is low because the
payload is produced in-process by the same build.
**Fix:** `?` the parse instead of `unwrap_or_default()`.

### E8 · medium · attachment finish events have no correlation key — confirmed
`ui/reader/ReaderFiles.kt:63-66`

```kotlin
if (done != null && !done.first) throw DownloadFailed(done.second.ifEmpty { "Download failed" })
```
`done` is simply "the next `Attachments` finish", which may belong to a *different* message's job. The user sees
an unrelated error string for an attachment that is fine, and `MAX_FINISH_WAITS = 5` waits (up to 10 minutes)
can be burned on other messages' jobs.
**Fix:** include the target (`folder_id`, `uid`) in the event or in a `MailNative.attachmentsResult(folderId, uid)`
read, and key the waiter on it.

### E9 · low · `createFolder`'s waiter can be fired by an unrelated `Folders` job — confirmed
`ui/state/MailStateFolders.kt:246` + `ui/state/MailState.kt:457` —
`finishWaiters.remove(kind)?.forEach { it(ok, e.optString("status")) }`. A "Refresh" finish that lands between
waiter registration and queue satisfies the create, so `FolderManagerScreen` shows the refresh's status (or an
unrelated failure) and clears it. Same class as E8.
**Fix:** add a correlation token to the event and match it.

### E10 · low · `spawn_flag_push` un-deduped; `spawn_push_after_grace` spawns an OS thread per action — confirmed
`crates/mailffi/src/net.rs:272-294` — `std::thread::spawn` + `sleep(grace+1)` per undoable action
(`api/mutate.rs:106,194`). N rapid archives = N sleeping threads plus N queued IMAP pushes.
**Fix:** coalesce on the existing in-flight table (same fix as D8).

### E11 · low · the list refresh is skipped while the shell is disposed `[corrected]`
`ui/shell/MailShell.kt:353-359`

```kotlin
onDispose { MailNotifier.onMailChanged = null }
```
**Correction:** the first draft claimed the notification is suppressed and the list is not refreshed. Wrong
on both halves — `MailNotifier.foreground` is set false in `MainActivity.kt:43` (`onStop`) and true in
`MainActivity.kt:38` (`onStart`), so whenever the shell is disposed the app is going to the background and
`plan` returns an action other than `"foreground"`: the notification **is** posted, and
`onMailChanged?.invoke()` is simply skipped. What actually happens is a possible redundant notification for
mail that arrived during an Activity recreate, plus a list that stays stale until the next resume — which
`ensureInit` already forces. So this is a cosmetic/latent note, not a lost-notification bug.
**Fix (optional):** drain a process-level "cache changed" flag in `ensureInit` instead of relying on the
callback slot.

### E12 · medium · `refreshSidebarRows()` runs a Rust SQL aggregate on the main thread — confirmed
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

### E13 · medium · the reader's reply strip overflows at 360dp / 150 % text scale *(unverified)*
`ui/reader/ReaderScreen.kt:291-299`

```kotlin
Row(modifier = Modifier.fillMaxWidth().height(48.dp), horizontalArrangement = Arrangement.SpaceEvenly) {
    ReplyAction(R.drawable.ic_reply, "Reply") { … }
    ReplyAction(R.drawable.ic_reply_all, "Reply all") { … }
    ReplyAction(R.drawable.ic_forward, "Forward") { … }
```
Three icon+label `TextButton`s with no `Modifier.weight` sum to ~330dp at 100 % scale and ~420dp at 150 %;
`Row` does not wrap, so the last action is squeezed to zero width and becomes unreachable. *(First draft said
"measured"; it was a size estimate only — this needs a device check.)*
**Fix:** `Modifier.weight(1f)` on each `ReplyAction` (or icons only / a `FlowRow` above a scale breakpoint),
and `heightIn(min = 48.dp)`.

### E14 · medium/low · JNI calls inside `remember` blocks (side effects in composition) — confirmed
- `ui/composer/ComposerScreen.kt:166-170` — `remember(editorBody) { … MailNative.editorDocument(…) }` builds
  the entire editor HTML document (body embedded, fresh nonce) **on the main thread**; the same build is done
  on `Dispatchers.IO` in `MailWebView.kt:128-133`. Medium.
- `ui/reader/ReaderScreen.kt:451-464` — `remember(m, dark, originalColors, scheme) { pagePaint(…) }` → two
  JNI calls. Low.
- `ui/list/ListScreen.kt:111-113` — `remember(state.folders) { state.folders.associate { it.id to state.deletePrompt(…) } }`. Low.
**Fix:** compute in a `LaunchedEffect` on IO, hold the result in state.

### E15 · low · `StatusStrip`'s tap line is 40dp tall — confirmed
`ui/shell/ShellBars.kt:237-257` — `.height(40.dp)` with the clickable line inside; below the 48dp touch-target
floor AGENTS.md requires.
**Fix:** `heightIn(min = 48.dp)`.

### E16 · low · stale `@SuppressLint` and unset `mixedContentMode` — confirmed
`ui/reader/MailWebView.kt:65` vs `ui/composer/ComposerEditor.kt:163` — `MailWebView` sets
`javaScriptEnabled = false` (`:143`), so the suppression is stale and hides accidental future changes.
`ComposerEditor` legitimately enables JS with `addJavascriptInterface`; it is safe only because
`mailcore::compose::editor::document` embeds `draft_editor_html`, which passes through
`html::sanitize_for_send` (allow-list, drops `script`), plus a nonce CSP, `allowFileAccess=false`,
`blockNetworkLoads=true` — none of which is asserted locally.
**Fix:** remove the stale annotation; set `mixedContentMode` explicitly and comment the `MCHost` contract.

### E17 · low · `usesCleartextTraffic="true"` app-wide with no `networkSecurityConfig` — confirmed
`AndroidManifest.xml:40` — the comment says cleartext is "opt-in per account", but the flag is global: with
`load_remote_images` on, an `http://` image from a plaintext-configured account's mail is fetched in the clear
by the reader WebView regardless of the account's TLS choice.
**Fix:** scope cleartext with `networkSecurityConfig` to the configured hosts, or reword the comment.

### E18 · low · possible double insets on the shell `Scaffold` *(very likely wrong — verify on device)*
`ui/shell/MailShell.kt:439-443` — the shell applies `Modifier.fillMaxSize().safeDrawingPadding()` while
`Scaffold`'s default `contentWindowInsets` also applies; the reader's own `Scaffold` opts out with
`contentWindowInsets = WindowInsets(0)`. A reviewer points out that Material3's `Scaffold` already subtracts
insets an ancestor has consumed, so the effect may not exist. Do not act on this without a device check; if it
does reproduce, the fix is `contentWindowInsets = WindowInsets(0, 0, 0, 0)` on the shell Scaffold.

### E19 · low · several screens never call `MailNative.ensureInit` — confirmed
`ui/contacts/ContactsScreen.kt:128,137`, `ui/outbox/OutboxScreen.kt:93`, the folder screens and
`MaintenanceSection`/`BackgroundStatus`. Only `MailShell` and the background components
(`MailCheckWorker`, `MailPushService`, `MailSchedule`, `MailActions`) initialise the core. Today the shell
always runs first, but any of these reached before the shell's `DisposableEffect` (or a future notification deep
link into a full-page route) would execute against the fallback `db_path()`
(`mailcore/src/db/mod.rs:18-25`), which on Android resolves to a relative path under `/` and fails.
**Fix:** initialise in `MailApplication.onCreate`.

### E20 · low · notification signature format re-implemented in Kotlin — confirmed
`MailNotifier.kt:118` duplicates `mailcore/src/sync/background/notify.rs:123-125`

```kotlin
out.put(tag, "$title\n$body")
// rust: pub fn signature_of(title: &str, body: &str) -> String { format!("{title}\n{body}") }
```
The two must match byte-for-byte or every posted notification reads as "changed" (or never changes).
**Fix:** expose `MailNative.signature(title, body)`, or have `plan()` return the per-tag signatures.

### E21 · low · the "Similar to: …" sentence template is duplicated in both frontends `[corrected]`
`ui/list/ListScreen.kt` / `ui/state/MailStateSearch.kt:79` and `crates/mailapp/qml/MessageList.qml:773`

```kotlin
similarLabel = "Similar to: ${subject.ifEmpty { "this message" }}"
// qml: text: qsTr("Similar to: %1").arg(root.similarSubject)
```
**Correction:** the first draft claimed the two frontends disagree on the empty-subject fallback. They do not —
`similar::target_subject` (`similar.rs:116-129`) already filters empties and returns `"(no subject)"`, so the
Kotlin `ifEmpty` is dead code and both show the same string. What really remains is that the sentence
**template** is written twice, so the two translations can drift apart independently — an i18n duplication,
not a behaviour bug.
**Fix:** `mailcore::similar::chip_label(db, …, locale) -> String`, or at minimum a shared golden test that
both templates render the same string for the same subject.

### E22 · low · Kotlin re-parses the core's `ReadTarget` JSON to find `account_id` — confirmed
`android/.../MailActions.kt:33` — `MailFlagWorker.enqueue(app, JSONObject(target).getLong("account_id"))`.
`ReadTarget` is a Rust struct (`notify.rs:132-135`); the WorkManager enqueue only needs the account id, so a
frontend must know an internal field name.
**Fix:** `MailNative.markReadAccount(target): Long`, or have `markRead` return `(report, account_id)`.

### E23 · low · `SHARED-CORE.md` items are open, stale, or wrongly filed `[corrected]`
Including two that the first draft proposed changing in the wrong direction.

- `ui/contacts/ContactsScreen.kt:77-87` (`Candidate.reasonText`) — Kotlin wording of the core's machine reasons,
  no Rust function exists (reasons come from `contacts::cleanup_candidates_json`).
  **Do not** move this to `mailcore`: `SHARED-CORE.md` §8 records it as a deliberate exception (the wording
  needs translation and tone control per frontend, while the *reason code* stays core-side). Leave it; keep the
  Rust label map in sync by test instead.
- `ui/reader/ReaderScreen.kt:317-321` (`canToggleColors`) — the "when the colours toggle shows" decision,
  Kotlin-only, as listed in SHARED-CORE. No action.
- `ui/settings/SettingLabels.kt:10-44` and `AccountSetupScreen.kt:532-536` (`securityLabel`) — a 10-key plus
  3-key value→words map in Kotlin, kept in step with the QML twins by convention only.
  **Do not** move these to `mailcore` either: value→display-string maps are locale data, and `SHARED-CORE.md`
  already flags them as deliberate; a translation test is the right guard. The real gap is that the *keys*
  come from `store::settings::choices`, which is core-side and correct — only the labels are duplicated.
- §2 ("Sync-on-resume gap … lives in Kotlin (`MailState.RESUME_SYNC_GAP_MS`)") is **stale**: the constant no
  longer exists; it is now `MailNative.resumeSyncDue` → `mailcore::sync::resume::resume_sync_due`.
  **Fix:** delete the stale entry so the file describes what is still duplicated.

### E24 · `[removed]` — there was nothing to fix
`ui/folders/FolderIcon.kt:17-25` maps the core's folder role to a drawable. It is not listed in
`SHARED-CORE.md`, so it is neither an approved exception nor a documented violation; role→icon is
unambiguously toolkit-side (a drawable id). No action.

### E25 · verified clean · no leaked parent `Global<JObject>`
`pushStart` (`android.rs:297-310`) drops the old monitor and its `Arc<KotlinListener>` before installing the new
one, and `MailPushService.onDestroy` calls `pushStop()`, so the service's global ref is released.

**Also verified clean in `android.rs`:** no panic path across the JNI boundary was confirmed, no local-reference
table overflow (no long JNI loops without `DeleteLocalRef`), no `GetStringUTFChars` read-after-release.

---

## F. Cross-frontend duplication (AGENTS.md §1 / §5 core-first)

| # | Logic | Qt side | Android side | Verdict |
|---|---|---|---|---|
| F1 | notification signature `"<title>\n<body>"` | — | `MailNotifier.kt:118` | real — core fn exists, unused (E20) |
| F2 | "Similar to: …" sentence | `MessageList.qml:773` | `MailStateSearch.kt:79` | template duplicated, behaviour identical; guard with a test, not a move (E21) |
| F3 | undo batch splitting | `Main.qml:1456-1464` JS | — | real — belongs in `mailcore` (D11) |
| F4 | pending-open payload decode | `Main.qml:777` JS | `MailActions.kt:33` | real — belongs in `mailcore` (D11, E22) |
| F5 | attachment filename decode | `MessageView.qml:376-384` JS | — | real — `mailcore::paths` exists (D11) |
| F6 | file:// URL building | `MessageView.qml:170-181` JS | — | real — `mailapp::bridge::messages::files` exists (D11) |
| F7 | account/security value → words | `AccountSetup.qml`/`Settings.qml` | `SettingLabels.kt:10-44` | **deliberate** — locale data, listed in SHARED-CORE; guard with a golden test (E23) |
| F8 | cleanup-candidate reason wording | `Contacts.qml` | `ContactsScreen.kt:77-87` | **deliberate** — SHARED-CORE §8 records the exception (E23) |

---

## Verification methodology

- **Read:** every file in `crates/mailcore/src`, `crates/mailapp/src`, `crates/mailapp/qml`,
  `crates/mailffi/src` (except generated `frb_generated.rs`), and `android/app/src/main` Kotlin,
  plus the four manifest/backup XML files, the root docs, and the jni 0.22.4 source.
- **`[verify]`** — reproduced by running code: C1 (real `domain_label` + `label[..4]`), C2 (exact algorithm,
  timing table), A1/A3/A11 (synthetic DBs + `schema_meta` variants), A2 (two-connection deferred-tx upgrade),
  A4 (32,766-variable statement), A7 (`attachment_has_data` on a missing id), A8 (`to_addrs='not json'`),
  A18 (`sqlite_master` diff + grep for `select *`), B1 (lettre `Timeout(None)` semantics).
  The `[verify]` tag appears on the item itself and means exactly this: the claim was executed, not merely read.
- **Not run:** `cargo fmt`/`clippy`/`cargo test`, `scripts/qml-check.sh`, `./build.sh --android`.
  Run the applicable one before closing each item.
- **Unverified claims flagged inline:** D3 (uid `i32` width on a real large mailbox), D10 (wrapping at runtime),
  E13 and E18 (layout measurements on a device), B11 (partial-move behaviour on a real server).
