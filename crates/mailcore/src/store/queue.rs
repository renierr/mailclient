//! Outbox (`send_queue`) for reliable sending.

use rusqlite::params;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{QueueStatus, QueuedSend};
use crate::store::now;

/// Enqueue a message for sending. `message_id` may point at a draft row.
pub fn enqueue(db: &Db, account_id: i64, message_id: Option<i64>) -> Result<i64> {
    let ts = now();
    db.conn().execute(
        "insert into send_queue (account_id, message_id, status, retries, created_at, updated_at)
         values (?1, ?2, 'queued', 0, ?3, ?3)",
        params![account_id, message_id, ts],
    )?;
    Ok(db.conn().last_insert_rowid())
}

/// All pending (queued/failed/sending) entries, oldest first.
pub fn list_pending(db: &Db) -> Result<Vec<QueuedSend>> {
    let mut stmt = db.conn().prepare(
        "select id, account_id, message_id, status, last_error, retries,
            created_at, updated_at from send_queue
         where status in ('queued', 'sending', 'failed')
         order by created_at",
    )?;
    let rows = stmt
        .query_map([], |row| {
            let status: String = row.get(3)?;
            Ok(QueuedSend {
                id: row.get(0)?,
                account_id: row.get(1)?,
                message_id: row.get(2)?,
                status: QueueStatus::parse_status(&status),
                last_error: row.get(4)?,
                retries: row.get::<_, i64>(5)? as u64,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
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

    #[test]
    fn enqueue_and_fail_then_sent() {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "a".to_string(),
                email_address: "a@x.y".to_string(),
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
        let id = enqueue(&db, acc, None).unwrap();
        assert_eq!(list_pending(&db).unwrap().len(), 1);
        mark_failed(&db, id, "connection refused").unwrap();
        let pending = list_pending(&db).unwrap();
        assert_eq!(pending[0].retries, 1);
        assert_eq!(pending[0].status, QueueStatus::Failed);
        mark_sent(&db, id).unwrap();
        assert!(list_pending(&db).unwrap().is_empty());
    }
}
