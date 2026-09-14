//! Outbox (`send_queue`) for reliable sending.

use rusqlite::params;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{QueueStatus, QueuedSend};
use crate::store::{json_vec, now};

/// Persist a fully-built MIME message for later (or immediate) SMTP submit.
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
         values (?1, ?2, 'queued', 0, ?3, ?4, ?5, ?6, ?6)",
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

/// Enqueue a message for sending. `message_id` may point at a draft row.
pub fn enqueue(db: &Db, account_id: i64, message_id: Option<i64>) -> Result<i64> {
    enqueue_mime(db, account_id, message_id, &[], "", &[])
}

fn row_to_queued(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueuedSend> {
    let status: String = row.get(3)?;
    let to_raw: String = row.get(8).unwrap_or_default();
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

/// Pending rows that have MIME bytes and can actually be submitted.
pub fn list_submittable(db: &Db, account_id: i64) -> Result<Vec<QueuedSend>> {
    let mut stmt = db.conn().prepare(&format!(
        "select {COLS} from send_queue
         where account_id = ?1
           and status in ('queued', 'sending', 'failed')
           and raw_mime is not null
           and length(raw_mime) > 0
         order by created_at"
    ))?;
    let rows = stmt
        .query_map([account_id], row_to_queued)?
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

/// Mark an entry as in-flight so a crash retries the same MIME.
pub fn mark_sending(db: &Db, id: i64) -> Result<()> {
    set_status(db, id, QueueStatus::Sending, None)
}

/// Crash recovery: `sending` rows never got a final status, so put them
/// back in `queued` for the next submit of the same MIME bytes.
pub fn requeue_interrupted(db: &Db) -> Result<u64> {
    let n = db.conn().execute(
        "update send_queue set status = 'queued', updated_at = ?1
         where status = 'sending'",
        [now()],
    )?;
    Ok(n as u64)
}

/// Mark an entry sent.
pub fn mark_sent(db: &Db, id: i64) -> Result<()> {
    set_status(db, id, QueueStatus::Sent, None)
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

fn set_status(db: &Db, id: i64, status: QueueStatus, error: Option<&str>) -> Result<()> {
    let rows = db.conn().execute(
        "update send_queue set status = ?1, last_error = ?2, updated_at = ?3 where id = ?4",
        params![status.as_str(), error, now(), id],
    )?;
    if rows == 0 {
        return Err(StoreError::NotFound(format!("queue entry {id}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAccount;
    use crate::store::accounts;

    fn setup() -> (Db, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "a".to_string(),
                email_address: "a@x.y".to_string(),
                from_name: String::new(),
                imap_host: "h".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "h".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        (db, acc)
    }

    #[test]
    fn enqueue_and_fail_then_sent() {
        let (db, acc) = setup();
        let id = enqueue(&db, acc, None).unwrap();
        assert_eq!(list_pending(&db).unwrap().len(), 1);
        mark_failed(&db, id, "connection refused").unwrap();
        let pending = list_pending(&db).unwrap();
        assert_eq!(pending[0].retries, 1);
        assert_eq!(pending[0].status, QueueStatus::Failed);
        mark_sent(&db, id).unwrap();
        assert!(list_pending(&db).unwrap().is_empty());
    }

    #[test]
    fn mime_bytes_roundtrip_and_submittable() {
        let (db, acc) = setup();
        let raw = b"From: a@x.y\r\nTo: b@x.y\r\nSubject: hi\r\n\r\nbody";
        let id = enqueue_mime(&db, acc, None, raw, "a@x.y", &["b@x.y".to_string()]).unwrap();
        mark_sending(&db, id).unwrap();
        let row = get(&db, id).unwrap();
        assert_eq!(row.status, QueueStatus::Sending);
        assert_eq!(row.raw_mime.as_deref(), Some(raw.as_slice()));
        assert_eq!(row.envelope_from.as_deref(), Some("a@x.y"));
        assert_eq!(row.envelope_to, vec!["b@x.y".to_string()]);
        assert_eq!(list_submittable(&db, acc).unwrap().len(), 1);
        enqueue(&db, acc, None).unwrap();
        assert_eq!(list_submittable(&db, acc).unwrap().len(), 1);
    }
}
