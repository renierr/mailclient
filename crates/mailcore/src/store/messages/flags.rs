//! Read / starred / draft state, and the queue that carries local changes
//! to the server.
//!
//! A click must not wait for IMAP, so a toggle writes locally and sets
//! `flags_dirty`; the next push sends it and clears the mark. Everything
//! that reads flags back from the server therefore has to respect that flag
//! or it would revert the user (see `set_flags_by_uid`).

use rusqlite::params;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::Message;
use crate::store::now;

use super::{row_to_message, COLS};

/// Flip read/starred flags, marking the row for the next server push.
///
/// The UI calls this on click and returns immediately; `flags_dirty` is what
/// keeps the change from being reverted by the next sync (see
/// [`list_flags_dirty`], [`clear_flags_dirty`]).
pub fn set_flags(db: &Db, id: i64, is_read: bool, is_starred: bool) -> Result<()> {
    let n = db.conn().execute(
        "update messages set is_read = ?1, is_starred = ?2, flags_dirty = 1,
            updated_at = ?3
         where id = ?4",
        params![i64::from(is_read), i64::from(is_starred), now(), id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound(format!("message {id}")));
    }
    Ok(())
}

/// Messages of an account whose flags still need pushing to the server.
pub fn list_flags_dirty(db: &Db, account_id: i64) -> Result<Vec<Message>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(&format!(
        "select {COLS} from messages
         where account_id = ?1 and flags_dirty = 1
         order by folder_id, uid"
    ))?;
    let rows = stmt.query_map([account_id], row_to_message)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Mark one message's flags as pushed — but only the state that was pushed.
///
/// A push is not instantaneous, and the user can click again while it is in
/// flight. Clearing unconditionally would drop that newer toggle on the
/// floor: it set `flags_dirty` back to 1, the clear wiped it, and the next
/// sync pulled the server's now-stale flags back over it. Matching on the
/// flags that actually went to the server leaves such a row dirty for the
/// next round instead. Returns `true` when the row was cleared.
pub fn clear_flags_dirty(
    db: &Db,
    id: i64,
    pushed_read: bool,
    pushed_starred: bool,
) -> Result<bool> {
    let n = db.conn().execute(
        "update messages set flags_dirty = 0
         where id = ?1 and is_read = ?2 and is_starred = ?3",
        params![id, i64::from(pushed_read), i64::from(pushed_starred)],
    )?;
    Ok(n > 0)
}

/// Update flags of one UID in a folder (no-op if unknown).
///
/// Sync calls this for every message in the window on every pass, and the
/// flags are almost always the ones already stored. The trailing inequality
/// makes that case update no rows at all, which keeps `updated_at` honest
/// and -- the reason it is here -- stops the FTS update trigger firing.
pub fn set_flags_by_uid(
    db: &Db,
    account_id: i64,
    folder_id: i64,
    uid: u32,
    is_read: bool,
    is_starred: bool,
    is_draft: bool,
) -> Result<()> {
    db.conn().execute(
        "update messages set is_read = ?1, is_starred = ?2, is_draft = ?3,
            updated_at = ?4
         where account_id = ?5 and folder_id = ?6 and uid = ?7 and flags_dirty = 0
           and (is_read <> ?1 or is_starred <> ?2 or is_draft <> ?3)",
        params![
            i64::from(is_read),
            i64::from(is_starred),
            i64::from(is_draft),
            now(),
            account_id,
            folder_id,
            uid as i64,
        ],
    )?;
    Ok(())
}

/// Run one statement across a set of UIDs, returning rows affected.
///
/// `sql_head` is the statement up to (and including) the `and` that precedes
/// the UID list; this appends `uid in (?, ?, …)` with one placeholder per
/// deduplicated UID. `leading` holds the parameters `sql_head` already refers
/// to, in `?1..?n` order — the UID bindings follow them.
///
/// An empty UID set is 0 rows, not an empty `in ()`, which is a syntax error.
fn execute_over_uids(
    db: &Db,
    sql_head: &str,
    mut leading: Vec<Box<dyn rusqlite::ToSql>>,
    uids: &[u32],
) -> Result<u64> {
    let mut clean: Vec<i64> = uids.iter().map(|u| i64::from(*u)).collect();
    clean.sort_unstable();
    clean.dedup();
    if clean.is_empty() {
        return Ok(0);
    }
    let placeholders = vec!["?"; clean.len()].join(",");
    let sql = format!("{sql_head} uid in ({placeholders})");
    leading.reserve(clean.len());
    for u in clean {
        leading.push(Box::new(u));
    }
    let refs: Vec<&dyn rusqlite::ToSql> = leading.iter().map(|b| b.as_ref()).collect();
    let n = db.conn().execute(&sql, refs.as_slice())?;
    Ok(n as u64)
}

/// Bulk mark read/unread for one folder (local-only, queued via
/// `flags_dirty` like the single-click path). Only the read flag moves —
/// starred state is preserved. Returns rows touched.
pub fn set_read_many_by_uids(db: &Db, folder_id: i64, uids: &[u32], read: bool) -> Result<u64> {
    execute_over_uids(
        db,
        "update messages set is_read = ?1, flags_dirty = 1, updated_at = ?2
         where folder_id = ?3 and",
        vec![
            Box::new(i64::from(read)),
            Box::new(now()),
            Box::new(folder_id),
        ],
        uids,
    )
}

/// Bulk star/unstar for one folder (local-only, queued). Only the starred
/// flag moves — read state is preserved. Returns rows touched.
pub fn set_star_many_by_uids(db: &Db, folder_id: i64, uids: &[u32], starred: bool) -> Result<u64> {
    execute_over_uids(
        db,
        "update messages set is_starred = ?1, flags_dirty = 1, updated_at = ?2
         where folder_id = ?3 and",
        vec![
            Box::new(i64::from(starred)),
            Box::new(now()),
            Box::new(folder_id),
        ],
        uids,
    )
}

/// Bulk delete cached rows of one folder by UID. Returns rows removed.
pub fn delete_many_by_uids(db: &Db, folder_id: i64, uids: &[u32]) -> Result<u64> {
    execute_over_uids(
        db,
        "delete from messages where folder_id = ?1 and",
        vec![Box::new(folder_id)],
        uids,
    )
}
