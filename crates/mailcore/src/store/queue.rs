//! Outbox (`send_queue`) for reliable sending.

use rusqlite::{params, OptionalExtension};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{QueueStatus, QueuedSend};
use crate::store::{json_vec_logged, now};

/// How often an outbox row may be retried before it is left alone. A row that
/// keeps failing is a permanent rejection (bad recipient, blocked sender), not
/// a transient outage, and retrying it forever only delays the rows behind it.
pub const MAX_SEND_RETRIES: u64 = 5;

/// How long a completed row is kept for diagnostics before it is pruned.
const SENT_RETENTION_DAYS: i64 = 30;

/// How long a `sending` row must sit untouched before a flush treats it as
/// orphaned by a crash. A live submit from another process (the GUI and the
/// `--sync-once` CLI share the DB) holds its row for seconds; requeueing it
/// early would hand the same MIME to a second submitter and deliver twice.
const STALE_SENDING_MINUTES: i64 = 30;

/// Timestamp `days` in the past, in the same format [`now`] writes — the
/// column is text, so a different encoding would compare wrong.
fn days_ago(days: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::days(days))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// How far ahead a scheduled send must lie: anything sooner is an immediate
/// send with extra steps, and clock skew between the picker's local time and
/// UTC storage would otherwise schedule in the past.
pub const SCHEDULE_LEAD_SECS: i64 = 60;

/// Parse any RFC3339 instant and normalize it to the shape [`now`] writes
/// (UTC, seconds), so stored values compare lexicographically. Rejects
/// instants that are not at least [`SCHEDULE_LEAD_SECS`] in the future.
pub fn normalize_future_send_at(raw: &str) -> Result<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map_err(|_| StoreError::InvalidInput("pick a valid date and time".to_string()))?;
    let utc = parsed.to_utc();
    let min = chrono::Utc::now() + chrono::Duration::seconds(SCHEDULE_LEAD_SECS);
    if utc <= min {
        return Err(StoreError::InvalidInput(
            "pick a time at least a minute in the future".to_string(),
        ));
    }
    Ok(utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Arguments for [`enqueue_scheduled_mime`]: the built message plus its time.
pub struct ScheduledMime<'a> {
    pub message_id: Option<i64>,
    pub raw_mime: &'a [u8],
    pub envelope_from: &'a str,
    pub envelope_to: &'a [String],
    pub request_dsn: bool,
    pub send_at: &'a str,
}

/// Persist a fully-built MIME message for delivery at `mime.send_at` (UTC
/// RFC3339, validated by [`normalize_future_send_at`]). The row waits as
/// `scheduled` — held back from every submit — until [`promote_due`] moves it
/// to `queued`. Unlike [`enqueue_mime`] it is born unclaimed: nothing is
/// submitting it yet.
pub fn enqueue_scheduled_mime(db: &Db, account_id: i64, mime: &ScheduledMime<'_>) -> Result<i64> {
    let send_at = normalize_future_send_at(mime.send_at)?;
    let ts = now();
    db.conn().execute(
        "insert into send_queue (account_id, message_id, status, retries,
            raw_mime, envelope_from, envelope_to, request_dsn, send_at, created_at, updated_at)
         values (?1, ?2, 'scheduled', 0, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![
            account_id,
            mime.message_id,
            mime.raw_mime,
            mime.envelope_from,
            serde_json::to_string(mime.envelope_to)?,
            mime.request_dsn,
            send_at,
            ts,
        ],
    )?;
    Ok(db.conn().last_insert_rowid())
}

/// Persist a fully-built MIME message for immediate SMTP submit. The row is
/// born `sending`, i.e. already claimed by the caller: a flush in another
/// process must not grab it between this insert and the caller's own submit.
/// A crash before that submit leaves it for [`requeue_interrupted`].
/// `request_dsn` asks the servers for a delivery confirmation on submit.
pub fn enqueue_mime(
    db: &Db,
    account_id: i64,
    message_id: Option<i64>,
    raw_mime: &[u8],
    envelope_from: &str,
    envelope_to: &[String],
    request_dsn: bool,
) -> Result<i64> {
    let ts = now();
    db.conn().execute(
        "insert into send_queue (account_id, message_id, status, retries,
            raw_mime, envelope_from, envelope_to, request_dsn, created_at, updated_at)
         values (?1, ?2, 'sending', 0, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![
            account_id,
            message_id,
            raw_mime,
            envelope_from,
            serde_json::to_string(envelope_to)?,
            request_dsn,
            ts,
        ],
    )?;
    Ok(db.conn().last_insert_rowid())
}

fn row_to_queued(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueuedSend> {
    let status: String = row.get(3)?;
    let to_raw: String = row.get(8)?;
    let id: i64 = row.get(0)?;
    Ok(QueuedSend {
        id,
        account_id: row.get(1)?,
        message_id: row.get(2)?,
        status: QueueStatus::parse_status(&status),
        last_error: row.get(4)?,
        retries: crate::store::int_col(row, 5)?,
        raw_mime: row.get(6)?,
        envelope_from: row.get(7)?,
        // Read as empty when corrupt, which `submit_claimed` refuses ("no
        // envelope recipients"): never sent to a guessed list.
        envelope_to: json_vec_logged(&to_raw, "queue entry", "envelope_to", id),
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        request_dsn: row.get(11)?,
        send_at: row.get(12)?,
    })
}

const COLS: &str = "id, account_id, message_id, status, last_error, retries,
    raw_mime, envelope_from, envelope_to, created_at, updated_at, request_dsn, send_at";

/// All pending (queued/sending/failed/scheduled) entries, oldest first.
pub fn list_pending(db: &Db) -> Result<Vec<QueuedSend>> {
    let mut stmt = db.conn().prepare(&format!(
        "select {COLS} from send_queue
         where status in ('queued', 'sending', 'failed', 'scheduled')
         order by created_at"
    ))?;
    let rows = stmt
        .query_map([], row_to_queued)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Unclaimed rows that still hold MIME bytes, have retries left and are due,
/// oldest first. A row whose bytes were dropped (the failure was already
/// reported to the user, who owns the retry) or that hit [`MAX_SEND_RETRIES`]
/// is skipped, and so is a `sending` row (someone owns it) and a `scheduled`
/// row whose time has not come ([`promote_due`] moves it to `queued` when it
/// has). Candidates only -- a submitter must still win [`claim`] before
/// touching SMTP.
pub fn list_submittable(db: &Db, account_id: i64) -> Result<Vec<QueuedSend>> {
    let due = now();
    let mut stmt = db.conn().prepare(&format!(
        "select {COLS} from send_queue
          where account_id = ?1
            and status in ('queued', 'failed')
            and raw_mime is not null
            and length(raw_mime) > 0
            and retries < ?2
            and (send_at is null or send_at <= ?3)
          order by created_at"
    ))?;
    let rows = stmt
        .query_map(
            params![account_id, MAX_SEND_RETRIES as i64, due],
            row_to_queued,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Fetch one outbox row.
pub fn get(db: &Db, id: i64) -> Result<QueuedSend> {
    db.conn()
        .query_row(
            &format!("select {COLS} from send_queue where id = ?1"),
            [id],
            row_to_queued,
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("queue entry {id}")))
}

/// Atomically take ownership of an unclaimed row (`queued`/`failed` to
/// `sending`). `false` means another submitter got there first -- or the row
/// is gone or already sent -- and the caller must leave it alone. This single
/// conditional update is what keeps two processes flushing the same outbox
/// from delivering one message twice.
pub fn claim(db: &Db, id: i64) -> Result<bool> {
    let rows = db.conn().execute(
        "update send_queue set status = 'sending', updated_at = ?1
         where id = ?2 and status in ('queued', 'failed')",
        params![now(), id],
    )?;
    Ok(rows == 1)
}

/// Crash recovery: a `sending` row untouched for longer than any live submit
/// takes never got a final status. If it still holds MIME bytes, put it back
/// in `queued` for the next submit. If its bytes were dropped (aborted before
/// submit), mark it `failed` so it is never presented as queued for sending.
/// Also marks any legacy `queued` rows missing bytes as `failed`.
/// Scoped per account so a flush never disturbs another account's rows; recent
/// rows are left to their owner.
pub fn requeue_interrupted(db: &Db, account_id: i64) -> Result<u64> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(STALE_SENDING_MINUTES))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let n = db.conn().execute(
        "update send_queue set status = 'queued', updated_at = ?1
         where status = 'sending' and account_id = ?2 and updated_at < ?3
           and raw_mime is not null and length(raw_mime) > 0",
        params![now(), account_id, cutoff],
    )?;
    db.conn().execute(
        "update send_queue set status = 'failed', last_error = 'aborted or missing message bytes', updated_at = ?1
          where account_id = ?2 and (raw_mime is null or length(raw_mime) = 0)
            and (status = 'queued' or (status = 'sending' and updated_at < ?3))",
        params![now(), account_id, cutoff],
    )?;
    Ok(n as u64)
}

/// Move due `scheduled` rows to `queued` so the normal submit path picks them
/// up. Scoped per account like [`requeue_interrupted`]; `send_at` is kept, so
/// delivery can refresh the MIME `Date:` to the actual send time. Returns how
/// many rows became due. Every flush runs this first, so a due mail goes out
/// with the next sync even when nothing re-armed an alarm for it.
pub fn promote_due(db: &Db, account_id: i64) -> Result<u64> {
    let due = now();
    let n = db.conn().execute(
        "update send_queue set status = 'queued', updated_at = ?1
          where status = 'scheduled' and account_id = ?2
            and send_at is not null and send_at <= ?3
            and raw_mime is not null and length(raw_mime) > 0",
        params![due, account_id, due],
    )?;
    if n > 0 {
        log::info!("outbox: {n} scheduled send(s) due");
    }
    Ok(n as u64)
}

/// Move a held row to a new future instant: validates `send_at` like
/// [`normalize_future_send_at`], requires bytes still present, and refuses
/// rows that are `sending` (a submit owns them) or `sent`. A previous failure
/// is cleared: the retry is a new intent, not the old error.
pub fn reschedule(db: &Db, id: i64, send_at: &str) -> Result<String> {
    let send_at = normalize_future_send_at(send_at)?;
    let row = get(db, id)?;
    if !matches!(
        row.status,
        QueueStatus::Scheduled | QueueStatus::Queued | QueueStatus::Failed
    ) {
        return Err(StoreError::InvalidInput(
            "this entry is already being sent".to_string(),
        ));
    }
    if row.raw_mime.as_deref().is_none_or(|b| b.is_empty()) {
        return Err(StoreError::InvalidInput(
            "this entry has no message data left; send it again by hand".to_string(),
        ));
    }
    db.conn().execute(
        "update send_queue set status = 'scheduled', send_at = ?1,
            last_error = null, updated_at = ?2 where id = ?3",
        params![send_at, now(), id],
    )?;
    Ok(send_at)
}

/// Send a `scheduled` row with the next sync instead of at its time: back to
/// `queued` with no `send_at`. Refuses rows that are `sending` or `sent`.
pub fn send_now(db: &Db, id: i64) -> Result<()> {
    let row = get(db, id)?;
    if !matches!(
        row.status,
        QueueStatus::Scheduled | QueueStatus::Queued | QueueStatus::Failed
    ) {
        return Err(StoreError::InvalidInput(
            "this entry is already being sent".to_string(),
        ));
    }
    db.conn().execute(
        "update send_queue set status = 'queued', send_at = null, updated_at = ?1
          where id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

/// Earliest held `scheduled` instant (UTC RFC3339) with bytes still present,
/// past-due included so a caller arms an immediate check for it. `None` when
/// nothing is scheduled. For exact-alarm planning on the composing device.
pub fn next_due(db: &Db) -> Result<Option<(i64, String)>> {
    db.conn()
        .query_row(
            "select account_id, send_at from send_queue
              where status = 'scheduled'
                and raw_mime is not null and length(raw_mime) > 0
                and send_at is not null
              order by send_at limit 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(Into::into)
}

/// [`next_due`] scoped to one account, for per-account outbox views.
pub fn next_due_for(db: &Db, account_id: i64) -> Result<Option<String>> {
    db.conn()
        .query_row(
            "select send_at from send_queue
              where status = 'scheduled' and account_id = ?1
                and raw_mime is not null and length(raw_mime) > 0
                and send_at is not null
              order by send_at limit 1",
            [account_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
}

/// Accounts holding `scheduled` rows whose time has come (bytes present).
/// A scheduled send is user intent, not a poll: the sender flushes these
/// regardless of check intervals, quiet hours or push/poll mode, so this
/// deliberately ignores everything [`crate::sync::background::schedule`]
/// gates on.
pub fn due_scheduled_account_ids(db: &Db) -> Result<Vec<i64>> {
    let mut stmt = db.conn().prepare(
        "select distinct account_id from send_queue
          where status = 'scheduled'
            and raw_mime is not null and length(raw_mime) > 0
            and send_at is not null and send_at <= ?1
          order by account_id",
    )?;
    let rows = stmt.query_map(params![now()], |row| row.get(0))?;
    rows.collect::<rusqlite::Result<Vec<i64>>>()
        .map_err(Into::into)
}

/// Replace a row's MIME bytes (a scheduled send's `Date:` refresh before
/// submit). The row stays in its status; only the bytes change.
pub fn update_raw_mime(db: &Db, id: i64, raw: &[u8]) -> Result<()> {
    db.conn().execute(
        "update send_queue set raw_mime = ?1, updated_at = ?2 where id = ?3",
        params![raw, now(), id],
    )?;
    Ok(())
}

/// Mark an entry sent and release its MIME bytes — they exist only to make a
/// retry possible, and a delivered message is never retried. Without this the
/// outbox keeps a full copy (attachments included) of every message ever sent.
pub fn mark_sent(db: &Db, id: i64) -> Result<()> {
    let rows = db.conn().execute(
        "update send_queue set status = 'sent', last_error = null,
            raw_mime = null, updated_at = ?1 where id = ?2",
        params![now(), id],
    )?;
    if rows == 0 {
        return Err(StoreError::NotFound(format!("queue entry {id}")));
    }
    Ok(())
}

/// Drop a row's MIME bytes without changing its status, so it is never
/// resubmitted on its own. Used when a failure was reported to the user: the
/// retry is theirs to make, and a silent one would deliver a duplicate.
pub fn discard_mime(db: &Db, id: i64) -> Result<()> {
    db.conn().execute(
        "update send_queue set raw_mime = null, updated_at = ?1 where id = ?2",
        params![now(), id],
    )?;
    Ok(())
}

/// A send the user was told failed: mark the row `failed` (keeping an error
/// already recorded, else `error`) and drop its bytes so no sync can deliver
/// it behind the user's back. Retries are not bumped — nothing was retried.
pub fn fail_and_discard(db: &Db, id: i64, error: &str) -> Result<()> {
    db.conn().execute(
        "update send_queue
            set last_error = case when status = 'failed' and last_error is not null
                                  then last_error else ?1 end,
                status = 'failed', raw_mime = null, updated_at = ?2
          where id = ?3",
        params![error, now(), id],
    )?;
    Ok(())
}

/// Remove one row whatever its status (a send whose job never started).
pub fn delete(db: &Db, id: i64) -> Result<()> {
    db.conn()
        .execute("delete from send_queue where id = ?1", [id])?;
    Ok(())
}

/// Mark an entry failed with an error description (bumps retry counter).
pub fn mark_failed(db: &Db, id: i64, error: &str) -> Result<()> {
    let rows = db.conn().execute(
        "update send_queue set status = 'failed', last_error = ?1,
            retries = retries + 1, updated_at = ?2 where id = ?3",
        params![error, now(), id],
    )?;
    if rows == 0 {
        return Err(StoreError::NotFound(format!("queue entry {id}")));
    }
    Ok(())
}

/// Delete completed rows past the retention window. They carry no MIME any
/// more, but the row count would still grow for the life of the profile.
pub fn prune_sent(db: &Db) -> Result<u64> {
    let n = db.conn().execute(
        "delete from send_queue where status = 'sent' and updated_at < ?1",
        [days_ago(SENT_RETENTION_DAYS)],
    )?;
    Ok(n as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAccount;
    use crate::store::accounts;

    const RAW: &[u8] = b"From: a@x.y\r\nTo: b@x.y\r\nSubject: hi\r\n\r\nbody";

    fn account(db: &Db, name: &str, vault_key: &str) -> i64 {
        accounts::create(
            db,
            &NewAccount {
                name: name.to_string(),
                email_address: format!("{name}@x.y"),
                from_name: String::new(),
                imap_host: "h".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "h".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: vault_key.to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap()
    }

    fn setup() -> (Db, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc = account(&db, "a", "k");
        (db, acc)
    }

    fn enqueue(db: &Db, acc: i64) -> i64 {
        enqueue_mime(db, acc, None, RAW, "a@x.y", &["b@x.y".to_string()], false).unwrap()
    }

    /// Backdate a row so it looks orphaned by a crash long ago.
    fn age(db: &Db, id: i64, minutes: i64) {
        let ts = (chrono::Utc::now() - chrono::Duration::minutes(minutes))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        db.conn()
            .execute(
                "update send_queue set updated_at = ?1 where id = ?2",
                params![ts, id],
            )
            .unwrap();
    }

    #[test]
    fn enqueue_and_fail_then_sent() {
        let (db, acc) = setup();
        let id = enqueue(&db, acc);
        assert_eq!(list_pending(&db).unwrap().len(), 1);
        mark_failed(&db, id, "connection refused").unwrap();
        let pending = list_pending(&db).unwrap();
        assert_eq!(pending[0].retries, 1);
        assert_eq!(pending[0].status, QueueStatus::Failed);
        mark_sent(&db, id).unwrap();
        assert!(list_pending(&db).unwrap().is_empty());
    }

    #[test]
    fn mime_bytes_roundtrip_and_born_claimed() {
        let (db, acc) = setup();
        let id = enqueue(&db, acc);
        let row = get(&db, id).unwrap();
        assert_eq!(row.status, QueueStatus::Sending);
        assert_eq!(row.raw_mime.as_deref(), Some(RAW));
        assert_eq!(row.envelope_from.as_deref(), Some("a@x.y"));
        assert_eq!(row.envelope_to, vec!["b@x.y".to_string()]);
        assert!(!row.request_dsn);
        let dsn = enqueue_mime(&db, acc, None, RAW, "a@x.y", &["b@x.y".to_string()], true).unwrap();
        assert!(get(&db, dsn).unwrap().request_dsn);
        // Owned by the enqueuer: a concurrent flush must not see it.
        assert!(list_submittable(&db, acc).unwrap().is_empty());
        assert!(!claim(&db, id).unwrap());
    }

    #[test]
    fn only_one_submitter_wins_the_claim() {
        let (db, acc) = setup();
        let id = enqueue(&db, acc);
        mark_failed(&db, id, "timeout").unwrap();
        assert_eq!(list_submittable(&db, acc).unwrap().len(), 1);
        assert!(claim(&db, id).unwrap());
        assert!(!claim(&db, id).unwrap());
        assert!(list_submittable(&db, acc).unwrap().is_empty());
    }

    #[test]
    fn a_live_send_is_not_requeued_but_an_orphan_is() {
        let (db, acc) = setup();
        let live = enqueue(&db, acc);
        let orphan = enqueue(&db, acc);
        age(&db, orphan, STALE_SENDING_MINUTES + 1);
        assert_eq!(requeue_interrupted(&db, acc).unwrap(), 1);
        assert_eq!(get(&db, live).unwrap().status, QueueStatus::Sending);
        assert_eq!(get(&db, orphan).unwrap().status, QueueStatus::Queued);
    }

    #[test]
    fn an_orphan_without_bytes_becomes_failed_not_queued() {
        let (db, acc) = setup();
        let orphan = enqueue(&db, acc);
        discard_mime(&db, orphan).unwrap();
        age(&db, orphan, STALE_SENDING_MINUTES + 1);
        assert_eq!(requeue_interrupted(&db, acc).unwrap(), 0);
        let row = get(&db, orphan).unwrap();
        assert_eq!(row.status, QueueStatus::Failed);
    }

    #[test]
    fn marking_sent_releases_the_mime() {
        let (db, acc) = setup();
        let id = enqueue(&db, acc);
        mark_sent(&db, id).unwrap();
        let row = get(&db, id).unwrap();
        assert_eq!(row.status, QueueStatus::Sent);
        assert_eq!(row.raw_mime, None);
    }

    #[test]
    fn discarded_mime_is_never_resubmitted() {
        let (db, acc) = setup();
        let id = enqueue(&db, acc);
        mark_failed(&db, id, "550 rejected").unwrap();
        discard_mime(&db, id).unwrap();
        assert!(list_submittable(&db, acc).unwrap().is_empty());
        // Still visible as a pending outbox problem, just not auto-retried.
        assert_eq!(list_pending(&db).unwrap().len(), 1);
    }

    #[test]
    fn retry_cap_stops_resubmission() {
        let (db, acc) = setup();
        let id = enqueue(&db, acc);
        for _ in 0..MAX_SEND_RETRIES {
            mark_failed(&db, id, "timeout").unwrap();
            if get(&db, id).unwrap().retries < MAX_SEND_RETRIES {
                assert_eq!(list_submittable(&db, acc).unwrap().len(), 1);
                assert!(claim(&db, id).unwrap());
            }
        }
        assert!(list_submittable(&db, acc).unwrap().is_empty());
    }

    #[test]
    fn requeue_interrupted_is_scoped_to_the_account() {
        let (db, acc) = setup();
        let other = account(&db, "b", "k2");
        let mine = enqueue(&db, acc);
        let theirs = enqueue(&db, other);
        age(&db, mine, STALE_SENDING_MINUTES + 1);
        age(&db, theirs, STALE_SENDING_MINUTES + 1);
        assert_eq!(requeue_interrupted(&db, acc).unwrap(), 1);
        assert_eq!(get(&db, mine).unwrap().status, QueueStatus::Queued);
        assert_eq!(get(&db, theirs).unwrap().status, QueueStatus::Sending);
    }

    #[test]
    fn prune_keeps_recent_sent_rows() {
        let (db, acc) = setup();
        let recent = enqueue(&db, acc);
        let old = enqueue(&db, acc);
        mark_sent(&db, recent).unwrap();
        mark_sent(&db, old).unwrap();
        db.conn()
            .execute(
                "update send_queue set updated_at = ?1 where id = ?2",
                params![days_ago(SENT_RETENTION_DAYS + 1), old],
            )
            .unwrap();
        assert_eq!(prune_sent(&db).unwrap(), 1);
        assert!(get(&db, recent).is_ok());
        assert!(get(&db, old).is_err());
    }

    fn future_ts(minutes: i64) -> String {
        (chrono::Utc::now() + chrono::Duration::minutes(minutes))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }

    fn schedule(db: &Db, acc: i64, minutes: i64) -> i64 {
        let at = future_ts(minutes);
        let to = vec!["b@x.y".to_string()];
        enqueue_scheduled_mime(
            db,
            acc,
            &ScheduledMime {
                message_id: None,
                raw_mime: RAW,
                envelope_from: "a@x.y",
                envelope_to: &to,
                request_dsn: false,
                send_at: &at,
            },
        )
        .unwrap()
    }

    #[test]
    fn scheduled_rows_wait_unclaimed_until_due() {
        let (db, acc) = setup();
        let id = schedule(&db, acc, 60);
        let row = get(&db, id).unwrap();
        assert_eq!(row.status, QueueStatus::Scheduled);
        assert!(row.send_at.is_some());
        // Held back: not submittable, not claimable, but listed as pending.
        assert!(list_submittable(&db, acc).unwrap().is_empty());
        assert!(!claim(&db, id).unwrap());
        assert_eq!(list_pending(&db).unwrap().len(), 1);
        // Future validation: past and near times are refused; other offsets
        // normalize to the same stored instant.
        assert!(normalize_future_send_at("not a time").is_err());
        assert!(normalize_future_send_at(&future_ts(-5)).is_err());
        assert!(normalize_future_send_at(&future_ts(0)).is_err());
        let utc = chrono::Utc::now() + chrono::Duration::minutes(90);
        let east = chrono::FixedOffset::east_opt(7200).unwrap();
        let shifted = utc
            .with_timezone(&east)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        assert!(shifted.ends_with("+02:00"));
        assert_eq!(
            normalize_future_send_at(&shifted).unwrap(),
            utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        );
    }

    #[test]
    fn promote_due_moves_only_past_rows_and_keeps_send_at() {
        let (db, acc) = setup();
        let due = schedule(&db, acc, 60);
        let later = schedule(&db, acc, 120);
        // Backdate one past its time: only it promotes.
        let past = (chrono::Utc::now() - chrono::Duration::minutes(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        db.conn()
            .execute(
                "update send_queue set send_at = ?1 where id = ?2",
                params![past, due],
            )
            .unwrap();
        assert_eq!(promote_due(&db, acc).unwrap(), 1);
        let row = get(&db, due).unwrap();
        assert_eq!(row.status, QueueStatus::Queued);
        assert_eq!(row.send_at.as_deref(), Some(past.as_str()));
        assert_eq!(get(&db, later).unwrap().status, QueueStatus::Scheduled);
        // The due row submits now; the future one still waits.
        assert_eq!(
            list_submittable(&db, acc)
                .unwrap()
                .iter()
                .map(|r| r.id)
                .collect::<Vec<_>>(),
            vec![due]
        );
    }

    #[test]
    fn reschedule_rejects_the_past_and_send_now_clears_the_time() {
        let (db, acc) = setup();
        let id = schedule(&db, acc, 60);
        assert!(reschedule(&db, id, &future_ts(-10)).is_err());
        let moved = reschedule(&db, id, &future_ts(180)).unwrap();
        let row = get(&db, id).unwrap();
        assert_eq!(row.status, QueueStatus::Scheduled);
        assert_eq!(row.send_at.as_deref(), Some(moved.as_str()));
        send_now(&db, id).unwrap();
        let row = get(&db, id).unwrap();
        assert_eq!(row.status, QueueStatus::Queued);
        assert_eq!(row.send_at, None);
        assert_eq!(list_submittable(&db, acc).unwrap().len(), 1);
        // A live send cannot be rescheduled or rushed.
        let live = enqueue(&db, acc);
        assert!(reschedule(&db, live, &future_ts(180)).is_err());
        assert!(send_now(&db, live).is_err());
    }

    #[test]
    fn next_due_reports_the_earliest_held_instant() {
        let (db, acc) = setup();
        assert_eq!(next_due(&db).unwrap(), None);
        assert_eq!(next_due_for(&db, acc).unwrap(), None);
        schedule(&db, acc, 120);
        let first = schedule(&db, acc, 60);
        let (aid, at) = next_due(&db).unwrap().unwrap();
        assert_eq!(aid, acc);
        assert_eq!(at, get(&db, first).unwrap().send_at.unwrap());
        assert_eq!(
            next_due_for(&db, acc).unwrap().as_deref(),
            Some(at.as_str())
        );
    }

    #[test]
    fn due_scheduled_lists_only_accounts_past_their_time() {
        let (db, acc) = setup();
        assert!(due_scheduled_account_ids(&db).unwrap().is_empty());
        schedule(&db, acc, 120);
        // Still in the future: nobody is due.
        assert!(due_scheduled_account_ids(&db).unwrap().is_empty());
        // Backdate one row past its time: its account is due.
        let past = (chrono::Utc::now() - chrono::Duration::minutes(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let id = schedule(&db, acc, 60);
        db.conn()
            .execute(
                "update send_queue set send_at = ?1 where id = ?2",
                params![past, id],
            )
            .unwrap();
        assert_eq!(due_scheduled_account_ids(&db).unwrap(), vec![acc]);
    }
}
