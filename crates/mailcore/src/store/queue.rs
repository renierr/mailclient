//! Outbox (`send_queue`) for reliable sending.

use rusqlite::params;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{QueueStatus, QueuedSend};
use crate::store::{json_vec, now};

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

/// Persist a fully-built MIME message for immediate SMTP submit. The row is
/// born `sending`, i.e. already claimed by the caller: a flush in another
/// process must not grab it between this insert and the caller's own submit.
/// A crash before that submit leaves it for [`requeue_interrupted`].
pub fn enqueue_mime(
    db: &Db,
    account_id: i64,
    message_id: Option<i64>,
    raw_mime: &[u8],
    envelope_from: &str,
    envelope_to: &[String],
) -> Result<i64> {
    let ts = now();
    db.conn().execute(
        "insert into send_queue (account_id, message_id, status, retries,
            raw_mime, envelope_from, envelope_to, created_at, updated_at)
         values (?1, ?2, 'sending', 0, ?3, ?4, ?5, ?6, ?6)",
        params![
            account_id,
            message_id,
            raw_mime,
            envelope_from,
            serde_json::to_string(envelope_to)?,
            ts,
        ],
    )?;
    Ok(db.conn().last_insert_rowid())
}

fn row_to_queued(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueuedSend> {
    let status: String = row.get(3)?;
    let to_raw: String = row.get(8)?;
    Ok(QueuedSend {
        id: row.get(0)?,
        account_id: row.get(1)?,
        message_id: row.get(2)?,
        status: QueueStatus::parse_status(&status),
        last_error: row.get(4)?,
        retries: row.get::<_, i64>(5)? as u64,
        raw_mime: row.get(6)?,
        envelope_from: row.get(7)?,
        envelope_to: json_vec(&to_raw).unwrap_or_default(),
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

const COLS: &str = "id, account_id, message_id, status, last_error, retries,
    raw_mime, envelope_from, envelope_to, created_at, updated_at";

/// All pending (queued/failed/sending) entries, oldest first.
pub fn list_pending(db: &Db) -> Result<Vec<QueuedSend>> {
    let mut stmt = db.conn().prepare(&format!(
        "select {COLS} from send_queue
         where status in ('queued', 'sending', 'failed')
         order by created_at"
    ))?;
    let rows = stmt
        .query_map([], row_to_queued)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Unclaimed rows that still hold MIME bytes and have retries left, oldest
/// first. A row whose bytes were dropped (the failure was already reported to
/// the user, who owns the retry) or that hit [`MAX_SEND_RETRIES`] is skipped,
/// and so is a `sending` row: someone owns it. Candidates only -- a submitter
/// must still win [`claim`] before touching SMTP.
pub fn list_submittable(db: &Db, account_id: i64) -> Result<Vec<QueuedSend>> {
    let mut stmt = db.conn().prepare(&format!(
        "select {COLS} from send_queue
         where account_id = ?1
           and status in ('queued', 'failed')
           and raw_mime is not null
           and length(raw_mime) > 0
           and retries < ?2
         order by created_at"
    ))?;
    let rows = stmt
        .query_map(params![account_id, MAX_SEND_RETRIES as i64], row_to_queued)?
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
        .map_err(|_| StoreError::NotFound(format!("queue entry {id}")))
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
/// takes never got a final status, so put it back in `queued` for the next
/// submit of the same MIME bytes. Scoped per account so a flush never
/// disturbs another account's rows; recent rows are left to their owner.
pub fn requeue_interrupted(db: &Db, account_id: i64) -> Result<u64> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(STALE_SENDING_MINUTES))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let n = db.conn().execute(
        "update send_queue set status = 'queued', updated_at = ?1
         where status = 'sending' and account_id = ?2 and updated_at < ?3",
        params![now(), account_id, cutoff],
    )?;
    Ok(n as u64)
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
        enqueue_mime(db, acc, None, RAW, "a@x.y", &["b@x.y".to_string()]).unwrap()
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
}
