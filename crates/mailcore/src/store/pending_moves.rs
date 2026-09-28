//! `pending_moves`: undoable moves waiting out their grace period.
//!
//! A queued message is hidden from every list and count (see
//! [`HIDDEN`]) but stays in `messages` untouched, because on the server it
//! has not moved yet. Undo deletes the row; the push deletes the message row
//! once the IMAP move succeeded, and the cascade takes this row with it.

use rusqlite::params;

use super::now;
use crate::db::Db;
use crate::error::Result;

/// SQL predicate that keeps pending messages out of the UI's lists, counts
/// and searches, for queries on the unaliased `messages` table (joined
/// queries spell it with `m.id`).
pub(crate) const HIDDEN: &str = "id not in (select message_id from pending_moves)";

/// What a pending row will do on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PendingAction {
    /// To the account's Trash (marked `\Seen` first).
    Trash,
    /// To the account's Archive, created if missing.
    Archive,
    /// To `dest_folder_id`.
    Move,
}

impl PendingAction {
    pub fn as_str(self) -> &'static str {
        match self {
            PendingAction::Trash => "trash",
            PendingAction::Archive => "archive",
            PendingAction::Move => "move",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "trash" => Some(PendingAction::Trash),
            "archive" => Some(PendingAction::Archive),
            "move" => Some(PendingAction::Move),
            _ => None,
        }
    }
}

/// One due row, joined with where its message sits now.
#[derive(Debug, Clone)]
pub struct PendingMove {
    pub message_id: i64,
    pub folder_id: i64,
    pub uid: u32,
    pub action: PendingAction,
    pub dest_folder_id: Option<i64>,
    pub attempts: i64,
}

/// Queue `message_ids` under one `batch` (the Undo handle). Re-queuing a
/// message replaces its earlier row. Returns the number queued.
pub fn queue(
    db: &Db,
    message_ids: &[i64],
    action: PendingAction,
    dest_folder_id: Option<i64>,
    batch: &str,
    due_at: &str,
) -> Result<u64> {
    let ts = now();
    let conn = db.conn();
    let tx = conn.unchecked_transaction()?;
    let mut n = 0u64;
    {
        let mut stmt = tx.prepare(
            "insert into pending_moves
                 (message_id, batch, action, dest_folder_id, due_at, attempts, created_at, updated_at)
             values (?1, ?2, ?3, ?4, ?5, 0, ?6, ?6)
             on conflict (message_id) do update set
                 batch = excluded.batch,
                 action = excluded.action,
                 dest_folder_id = excluded.dest_folder_id,
                 due_at = excluded.due_at,
                 attempts = 0,
                 updated_at = excluded.updated_at",
        )?;
        for id in message_ids {
            n += stmt.execute(params![
                id,
                batch,
                action.as_str(),
                dest_folder_id,
                due_at,
                ts
            ])? as u64;
        }
    }
    tx.commit()?;
    Ok(n)
}

/// Undo: drop every row of `batch`. Returns how many came back; 0 means the
/// batch was already pushed (or never existed).
pub fn cancel_batch(db: &Db, batch: &str) -> Result<u64> {
    Ok(db
        .conn()
        .execute("delete from pending_moves where batch = ?1", [batch])? as u64)
}

/// Rows of one account whose grace period ended by `now`, oldest first.
pub fn list_due(db: &Db, account_id: i64, now: &str) -> Result<Vec<PendingMove>> {
    let mut stmt = db.conn().prepare(
        "select p.message_id, m.folder_id, m.uid, p.action, p.dest_folder_id, p.attempts
         from pending_moves p
         join messages m on m.id = p.message_id
         where m.account_id = ?1 and p.due_at <= ?2
         order by p.due_at, p.message_id",
    )?;
    let rows = stmt
        .query_map(params![account_id, now], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<i64>>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(message_id, folder_id, uid, action, dest, attempts)| {
            Some(PendingMove {
                message_id,
                folder_id,
                uid: uid as u32,
                action: PendingAction::parse(&action)?,
                dest_folder_id: dest,
                attempts,
            })
        })
        .collect())
}

/// Whether the account has any pending row at all, due or not.
pub fn any_for_account(db: &Db, account_id: i64) -> Result<bool> {
    let n: i64 = db.conn().query_row(
        "select count(*) from pending_moves p
         join messages m on m.id = p.message_id
         where m.account_id = ?1",
        [account_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Count a failed push attempt.
pub fn record_failure(db: &Db, message_ids: &[i64]) -> Result<()> {
    let ts = now();
    for id in message_ids {
        db.conn().execute(
            "update pending_moves set attempts = attempts + 1, updated_at = ?2
             where message_id = ?1",
            params![id, ts],
        )?;
    }
    Ok(())
}

/// Give up on rows: the messages show again where they were.
pub fn remove(db: &Db, message_ids: &[i64]) -> Result<()> {
    for id in message_ids {
        db.conn()
            .execute("delete from pending_moves where message_id = ?1", [id])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount};
    use crate::store::{accounts, folders, messages};

    fn setup() -> (Db, i64, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "t".to_string(),
                email_address: "me@example.com".to_string(),
                from_name: String::new(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                smtp_security: "starttls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 300,
            },
        )
        .unwrap();
        let inbox = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        (db, acc, inbox)
    }

    #[test]
    fn queued_rows_come_due_and_undo_drops_the_whole_batch() {
        let (db, acc, inbox) = setup();
        let a = messages::upsert(&db, &messages::sample_new(acc, inbox, 1)).unwrap();
        let b = messages::upsert(&db, &messages::sample_new(acc, inbox, 2)).unwrap();
        let due = "2000-01-01T00:00:10Z";
        assert_eq!(
            queue(&db, &[a, b], PendingAction::Trash, None, "b1", due).unwrap(),
            2
        );
        assert!(any_for_account(&db, acc).unwrap());
        assert!(list_due(&db, acc, "2000-01-01T00:00:09Z")
            .unwrap()
            .is_empty());
        let rows = list_due(&db, acc, due).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].action, PendingAction::Trash);
        assert_eq!(rows[0].uid, 1);

        assert_eq!(cancel_batch(&db, "b1").unwrap(), 2);
        assert_eq!(cancel_batch(&db, "b1").unwrap(), 0);
        assert!(!any_for_account(&db, acc).unwrap());
    }

    #[test]
    fn a_pushed_message_takes_its_row_with_it() {
        let (db, acc, inbox) = setup();
        let a = messages::upsert(&db, &messages::sample_new(acc, inbox, 1)).unwrap();
        queue(
            &db,
            &[a],
            PendingAction::Archive,
            None,
            "b",
            "2000-01-01T00:00:00Z",
        )
        .unwrap();
        messages::delete(&db, a).unwrap();
        assert!(!any_for_account(&db, acc).unwrap());
    }

    #[test]
    fn a_sync_upsert_keeps_the_message_pending() {
        let (db, acc, inbox) = setup();
        let a = messages::upsert(&db, &messages::sample_new(acc, inbox, 1)).unwrap();
        queue(
            &db,
            &[a],
            PendingAction::Trash,
            None,
            "b",
            "2000-01-01T00:00:00Z",
        )
        .unwrap();
        // The server still has it in INBOX, so the next sync upserts it again.
        messages::upsert(&db, &messages::sample_new(acc, inbox, 1)).unwrap();
        assert_eq!(list_due(&db, acc, "2000-01-01T00:00:00Z").unwrap().len(), 1);
    }
}
