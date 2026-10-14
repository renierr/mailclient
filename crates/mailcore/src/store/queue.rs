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
    /// Mail a delivery receipt to the account's own address once the row
    /// goes out. Never set on the receipt itself, so it cannot chain.
    pub notify_self: bool,
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
            raw_mime, envelope_from, envelope_to, request_dsn, notify_self, send_at, created_at, updated_at)
         values (?1, ?2, 'scheduled', 0, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![
            account_id,
            mime.message_id,
            mime.raw_mime,
            mime.envelope_from,
            serde_json::to_string(mime.envelope_to)?,
            mime.request_dsn,
            mime.notify_self,
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
        notify_self: row.get(13)?,
        next_attempt_at: row.get(14)?,
    })
}

const COLS: &str = "id, account_id, message_id, status, last_error, retries,
    raw_mime, envelope_from, envelope_to, created_at, updated_at, request_dsn, send_at, notify_self,
    next_attempt_at";

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
/// is skipped, and so is a `sending` row (someone owns it), a `scheduled`
/// row whose time has not come ([`promote_due`] moves it to `queued` when it
/// has) and a failed scheduled send still in its pause
/// ([`fail_scheduled_attempt`]): no sync, timer or alarm retries it early.
/// Candidates only -- a submitter must still win [`claim`] before touching
/// SMTP.
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
            and (next_attempt_at is null or next_attempt_at <= ?3)
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

/// Pauses before the second, third, … attempt of a scheduled send that
/// failed, whatever the cause (offline, server down, password missing). They
/// grow so a send that cannot go out costs a handful of wakeups, not one
/// every few minutes; after the last pause the next failure gives up
/// ([`MAX_SEND_RETRIES`] attempts in all).
pub const SCHEDULED_BACKOFF_SECS: [i64; 4] = [5 * 60, 15 * 60, 60 * 60, 3 * 60 * 60];

/// A scheduled send still failing this long after its time gives up at the
/// next failure, however many attempts it had: a mail meant for Tuesday
/// morning must not trickle out on Thursday. The first attempt always runs,
/// even when the device was off past the deadline.
pub const SCHEDULED_DEADLINE_SECS: i64 = 24 * 60 * 60;

/// What [`fail_scheduled_attempt`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduledFailure {
    /// Tries again at this UTC instant.
    RetryAt(String),
    /// Out of attempts, past the deadline, or refused for good: the row
    /// stays `failed` and dead until the user acts on it.
    GaveUp,
}

/// Record one failed attempt of a scheduled send (a row with `send_at`):
/// `failed` with the error, then either a pause from
/// [`SCHEDULED_BACKOFF_SECS`] or, when `permanent` (the server refused the
/// mail for good), out of attempts or past [`SCHEDULED_DEADLINE_SECS`], a
/// give-up — retries set to [`MAX_SEND_RETRIES`] so nothing retries it, and
/// `failure_noticed` cleared so [`take_failure_notices`] reports it once.
pub fn fail_scheduled_attempt(
    db: &Db,
    id: i64,
    error: &str,
    permanent: bool,
) -> Result<ScheduledFailure> {
    let row = get(db, id)?;
    let attempts = row.retries + 1;
    let now_t = chrono::Utc::now();
    let past_deadline = row
        .send_at
        .as_deref()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .is_some_and(|at| now_t > at.to_utc() + chrono::Duration::seconds(SCHEDULED_DEADLINE_SECS));
    let pause = SCHEDULED_BACKOFF_SECS.get((attempts - 1) as usize).copied();
    let decision = match pause {
        Some(secs) if !permanent && !past_deadline && attempts < MAX_SEND_RETRIES => {
            ScheduledFailure::RetryAt(
                (now_t + chrono::Duration::seconds(secs))
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            )
        }
        _ => ScheduledFailure::GaveUp,
    };
    let (retries, next_at) = match &decision {
        ScheduledFailure::RetryAt(at) => (attempts, Some(at.as_str())),
        ScheduledFailure::GaveUp => (MAX_SEND_RETRIES, None),
    };
    db.conn().execute(
        "update send_queue set status = 'failed', last_error = ?1, retries = ?2,
            next_attempt_at = ?3, failure_noticed = 0, updated_at = ?4
          where id = ?5",
        params![error, retries as i64, next_at, now(), id],
    )?;
    if decision == ScheduledFailure::GaveUp {
        log::info!("outbox: scheduled send {id} gave up after {attempts} attempt(s)");
    }
    Ok(decision)
}

/// A flush could not even try this account (its password or settings are
/// unreadable): every scheduled send of it that is due now counts one
/// failed attempt with `error`, so it backs off and finally gives up like a
/// send the server refused instead of being retried on every run.
pub fn fail_due_scheduled(db: &Db, account_id: i64, error: &str) -> Result<u64> {
    let now_s = now();
    let ids: Vec<i64> = {
        let mut stmt = db.conn().prepare(&format!(
            "select id from send_queue
              where account_id = ?1 and send_at is not null and status in {HELD_STATUSES}
                and raw_mime is not null and length(raw_mime) > 0
                and retries < ?2 and send_at <= ?3
                and (next_attempt_at is null or next_attempt_at <= ?3)"
        ))?;
        let rows = stmt.query_map(params![account_id, MAX_SEND_RETRIES as i64, now_s], |r| {
            r.get(0)
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for id in &ids {
        fail_scheduled_attempt(db, *id, error, false)?;
    }
    Ok(ids.len() as u64)
}

/// A scheduled send that gave up, reported once (Android notification, Qt
/// status line). Subject only — never recipients or the body (AGENTS §6
/// keeps those out of logs; a notification is visible on the lock screen).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FailedSend {
    pub queue_id: i64,
    pub account_id: i64,
    pub subject: String,
    pub error: String,
    /// The give-up in one line, phrased once for the Qt status line and the
    /// Android notification ([`FailedSend::line_for`]).
    pub line: String,
}

impl FailedSend {
    /// "Scheduled mail not sent: <subject> — <reason>".
    #[must_use]
    pub fn line_for(subject: &str, error: &str) -> String {
        if error.is_empty() {
            format!("Scheduled mail not sent: {subject}")
        } else {
            format!("Scheduled mail not sent: {subject} — {error}")
        }
    }
}

/// Scheduled sends that gave up and were not reported yet; each is marked
/// reported here, so a second call returns nothing new. Rescheduling or
/// Send now clears the mark again (a new attempt can fail anew).
pub fn take_failure_notices(db: &Db) -> Result<Vec<FailedSend>> {
    // id, account, MIME bytes (for the subject), last error.
    type NoticeRow = (i64, i64, Option<Vec<u8>>, Option<String>);
    let rows: Vec<NoticeRow> = {
        let mut stmt = db.conn().prepare(
            "select id, account_id, raw_mime, last_error from send_queue
              where status = 'failed' and send_at is not null
                and retries >= ?1 and failure_noticed = 0
              order by updated_at",
        )?;
        let mapped = stmt.query_map(params![MAX_SEND_RETRIES as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        mapped.collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::with_capacity(rows.len());
    for (id, account_id, raw, error) in rows {
        db.conn().execute(
            "update send_queue set failure_noticed = 1 where id = ?1",
            [id],
        )?;
        let subject = raw
            .as_deref()
            .and_then(crate::outbox::subject_from_mime)
            .unwrap_or_else(|| "(no subject)".to_string());
        let error = error.unwrap_or_default();
        out.push(FailedSend {
            queue_id: id,
            account_id,
            line: FailedSend::line_for(&subject, &error),
            subject,
            error,
        });
    }
    Ok(out)
}

/// Statuses a scheduled send can be in while it still waits to go out:
/// held (`scheduled`), promoted by a flush (`queued`), or failed after its
/// time with retries left (`failed`). Reschedule and Send now act on these.
const HELD_STATUSES: &str = "('scheduled', 'queued', 'failed')";

/// Why a conditional update on outbox row `id` of `account_id` touched
/// nothing: the row is gone (or another account's), a submit owns it, or its
/// bytes were dropped.
fn unchanged_reason(db: &Db, account_id: i64, id: i64) -> StoreError {
    match get(db, id) {
        Ok(row) if row.account_id != account_id => StoreError::NotFound("outbox entry".to_string()),
        Ok(row)
            if matches!(
                row.status,
                QueueStatus::Scheduled | QueueStatus::Queued | QueueStatus::Failed
            ) =>
        {
            StoreError::InvalidInput(
                "this entry has no message data left; send it again by hand".to_string(),
            )
        }
        Ok(_) => StoreError::InvalidInput("this entry is already being sent".to_string()),
        Err(e) => e,
    }
}

/// Move a waiting row of `account_id` to a new future instant: validates
/// `send_at` like [`normalize_future_send_at`], requires bytes still present,
/// and refuses rows that are `sending` (a submit owns them) or `sent`. The
/// check is part of the update itself, so a flush claiming the row at the
/// same moment either wins (and this fails) or loses (and the row waits).
/// A previous failure and its retry count are cleared: the new time is a new
/// intent, not the old error.
pub fn reschedule(db: &Db, account_id: i64, id: i64, send_at: &str) -> Result<String> {
    let send_at = normalize_future_send_at(send_at)?;
    let n = db.conn().execute(
        &format!(
            "update send_queue set status = 'scheduled', send_at = ?1, retries = 0,
                last_error = null, next_attempt_at = null, failure_noticed = 0, updated_at = ?2
              where id = ?3 and account_id = ?4 and status in {HELD_STATUSES}
                and raw_mime is not null and length(raw_mime) > 0"
        ),
        params![send_at, now(), id, account_id],
    )?;
    if n == 0 {
        return Err(unchanged_reason(db, account_id, id));
    }
    Ok(send_at)
}

/// Send a waiting row of `account_id` now instead of at its time: `queued`
/// with `send_at` set to now, so delivery still refreshes the `Date:` the
/// message was built with and the planners see it as due at once (an alarm
/// or due timer flushes it without waiting for a sync). Same guards and
/// reset as [`reschedule`].
pub fn send_now(db: &Db, account_id: i64, id: i64) -> Result<()> {
    let ts = now();
    let n = db.conn().execute(
        &format!(
            "update send_queue set status = 'queued', send_at = ?1, retries = 0,
                last_error = null, next_attempt_at = null, failure_noticed = 0, updated_at = ?1
              where id = ?2 and account_id = ?3 and status in {HELD_STATUSES}
                and raw_mime is not null and length(raw_mime) > 0"
        ),
        params![ts, id, account_id],
    )?;
    if n == 0 {
        return Err(unchanged_reason(db, account_id, id));
    }
    Ok(())
}

/// Every scheduled send still waiting to go out, as `(account_id, next
/// attempt)`. A held row's attempt is its `send_at`; a promoted one's is
/// `send_at` too (due now); one that failed waits for its
/// `next_attempt_at` ([`fail_scheduled_attempt`]). Rows without bytes or out
/// of retries are dead and left to the outbox list.
fn waiting_scheduled(db: &Db) -> Result<Vec<(i64, chrono::DateTime<chrono::Utc>)>> {
    let mut stmt = db.conn().prepare(&format!(
        "select account_id, send_at, next_attempt_at from send_queue
          where send_at is not null and status in {HELD_STATUSES}
            and raw_mime is not null and length(raw_mime) > 0
            and retries < ?1"
    ))?;
    let rows = stmt.query_map(params![MAX_SEND_RETRIES as i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;
    let parse = |raw: &str| {
        chrono::DateTime::parse_from_rfc3339(raw)
            .ok()
            .map(|t| t.to_utc())
    };
    let mut out = Vec::new();
    for row in rows {
        let (account_id, send_at, next_attempt) = row?;
        let Some(at) = parse(&send_at) else {
            log::warn!("outbox: unreadable send_at on a scheduled row of account {account_id}");
            continue;
        };
        let next = next_attempt
            .as_deref()
            .and_then(parse)
            .map_or(at, |t| t.max(at));
        out.push((account_id, next));
    }
    Ok(out)
}

/// When the earliest waiting scheduled send wants a flush (see
/// [`waiting_scheduled`]), past instants included so a caller flushes at
/// once. `None` when nothing waits. Due timers and exact alarms plan from
/// this.
pub fn next_due(db: &Db) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    Ok(waiting_scheduled(db)?.into_iter().map(|(_, at)| at).min())
}

/// Accounts with a waiting scheduled send whose attempt is due now. A
/// scheduled send is user intent, not a poll: the sender flushes these
/// regardless of check intervals, quiet hours or push/poll mode, so this
/// deliberately ignores everything [`crate::sync::background::schedule`]
/// gates on.
pub fn due_scheduled_account_ids(db: &Db) -> Result<Vec<i64>> {
    let now = chrono::Utc::now();
    let mut ids: Vec<i64> = waiting_scheduled(db)?
        .into_iter()
        .filter(|(_, at)| *at <= now)
        .map(|(id, _)| id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
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
mod tests;
