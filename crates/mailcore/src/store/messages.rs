//! The `messages` table: rows in, rows out, rows gone.
//!
//! Flag state and the attachment rows hanging off a message each have their
//! own rules, so they live next door: [`flags`] owns read/starred/draft and
//! the local-change queue, [`attachments`] owns the files.

mod attachments;
mod flags;

pub use attachments::{
    add_attachment, attachment_has_data, delete_attachments_for_message, get_attachment,
    list_attachments, replace_attachments, save_attachment_to_path, set_has_attachments,
};
pub use flags::{
    clear_flags_dirty, delete_many_by_uids, list_flags_dirty, set_flags, set_flags_by_uid,
    set_read_many_by_uids, set_star_many_by_uids,
};

use std::collections::HashMap;

use rusqlite::{params, OptionalExtension};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Message, NewMessage};
use crate::store::{json_vec, now, opt_bool};

pub(super) fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let to: String = row.get(8)?;
    let cc: String = row.get(9)?;
    let bcc: String = row.get(10)?;
    let kw: String = row.get(18)?;
    Ok(Message {
        id: row.get(0)?,
        account_id: row.get(1)?,
        folder_id: row.get(2)?,
        uid: row.get::<_, i64>(3)? as u32,
        message_id_header: row.get(4)?,
        thread_id: row.get(5)?,
        subject: row.get(6)?,
        from_addr: row.get(7)?,
        to_addrs: json_vec(&to).unwrap_or_default(),
        cc_addrs: json_vec(&cc).unwrap_or_default(),
        bcc_addrs: json_vec(&bcc).unwrap_or_default(),
        reply_to: row.get(11)?,
        date: row.get(12)?,
        snippet: row.get(13)?,
        body_text: row.get(14)?,
        body_html: row.get(15)?,
        raw_headers: row.get(16)?,
        is_read: opt_bool(row.get::<_, i64>(17)?),
        is_starred: opt_bool(row.get::<_, i64>(19)?),
        is_draft: opt_bool(row.get::<_, i64>(20)?),
        has_attachments: opt_bool(row.get::<_, i64>(21)?),
        keywords: json_vec(&kw).unwrap_or_default(),
        size: row.get::<_, i64>(22)? as u64,
        downloaded_full: opt_bool(row.get::<_, i64>(23)?),
    })
}

// Column order must match row_to_message indices.
pub(super) const COLS: &str = "id, account_id, folder_id, uid, message_id_header, thread_id,
    subject, from_addr, to_addrs, cc_addrs, bcc_addrs, reply_to, date, snippet,
     body_text, body_html, raw_headers, is_read, keywords, is_starred, is_draft,
    has_attachments, size, downloaded_full";

/// Insert a message, or replace it if the same `(account, folder, uid)` exists.
pub fn upsert(db: &Db, m: &NewMessage) -> Result<i64> {
    let ts = now();
    db.conn().execute(
        "insert into messages (account_id, folder_id, uid, message_id_header,
            thread_id, subject, from_addr, to_addrs, cc_addrs, bcc_addrs,
             reply_to, date, snippet, body_text, body_html, raw_headers, is_read, keywords,
            is_starred, is_draft, has_attachments, size, downloaded_full,
            created_at, updated_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
             ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?24)
          on conflict (account_id, folder_id, uid) do update set
            message_id_header = excluded.message_id_header,
            thread_id = excluded.thread_id,
            subject = excluded.subject,
            from_addr = excluded.from_addr,
            to_addrs = excluded.to_addrs,
            cc_addrs = excluded.cc_addrs,
            bcc_addrs = excluded.bcc_addrs,
            reply_to = excluded.reply_to,
            date = excluded.date,
            snippet = excluded.snippet,
            body_text = excluded.body_text,
             body_html = excluded.body_html,
             raw_headers = excluded.raw_headers,
            is_read = case when messages.flags_dirty != 0 then messages.is_read else excluded.is_read end,
            keywords = excluded.keywords,
            is_starred = case when messages.flags_dirty != 0 then messages.is_starred else excluded.is_starred end,
            is_draft = excluded.is_draft,
            has_attachments = excluded.has_attachments,
            size = excluded.size,
            downloaded_full = excluded.downloaded_full,
            updated_at = excluded.updated_at",
        params![
            m.account_id,
            m.folder_id,
            m.uid as i64,
            m.message_id_header,
            m.thread_id,
            m.subject,
            m.from_addr,
            serde_json::to_string(&m.to_addrs)?,
            serde_json::to_string(&m.cc_addrs)?,
            serde_json::to_string(&m.bcc_addrs)?,
            m.reply_to,
            m.date,
            m.snippet,
            m.body_text,
            m.body_html,
            m.raw_headers,
            i64::from(m.is_read),
            serde_json::to_string(&m.keywords)?,
            i64::from(m.is_starred),
            i64::from(m.is_draft),
            i64::from(m.has_attachments),
            m.size as i64,
            i64::from(m.downloaded_full),
            ts,
        ],
    )?;
    let id: i64 = db.conn().query_row(
        "select id from messages where account_id = ?1 and folder_id = ?2 and uid = ?3",
        params![m.account_id, m.folder_id, m.uid as i64],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// Compact message metadata for list views (no bodies, raw headers, or JSON arrays).
#[derive(Debug, Clone)]
pub struct CompactMessage {
    pub uid: u32,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    pub date: Option<String>,
    pub snippet: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
}

/// Builds the SQL `ORDER BY` clause for message listings.
///
/// `sort_field` is allowlisted (`date` | `from` | `subject`, anything else =
/// `date`). `descending` flips the primary key; `from`/`subject` keep newest-first
/// as the stable secondary order. The Date view orders by IMAP UID, which reflects
/// the server's delivery order.
fn folder_sort_clause(sort_field: &str, descending: bool) -> String {
    let dir = if descending { "desc" } else { "asc" };
    match sort_field.trim().to_ascii_lowercase().as_str() {
        "from" | "from_addr" | "sender" => {
            format!("coalesce(from_addr, '') collate nocase {dir}, date desc, id desc")
        }
        "subject" => {
            format!("coalesce(subject, '') collate nocase {dir}, date desc, id desc")
        }
        _ => format!("uid {dir}"),
    }
}

/// Lightweight query for folder message lists: selects only the 8 columns
/// needed for compact rows, skipping heavy bodies, raw headers, and JSON arrays.
pub fn list_compact_by_folder_sorted(
    db: &Db,
    folder_id: i64,
    limit: u64,
    offset: u64,
    sort_field: &str,
    descending: bool,
) -> Result<Vec<CompactMessage>> {
    let order = folder_sort_clause(sort_field, descending);
    let mut stmt = db.conn().prepare(&format!(
        "select uid, subject, from_addr, date, snippet, is_read, is_starred, has_attachments
         from messages where folder_id = ?1
         order by {order} limit ?2 offset ?3"
    ))?;
    let rows = stmt
        .query_map(params![folder_id, limit as i64, offset as i64], |row| {
            Ok(CompactMessage {
                uid: row.get::<_, i64>(0)? as u32,
                subject: row.get(1)?,
                from_addr: row.get(2)?,
                date: row.get(3)?,
                snippet: row.get(4)?,
                is_read: opt_bool(row.get::<_, i64>(5)?),
                is_starred: opt_bool(row.get::<_, i64>(6)?),
                has_attachments: opt_bool(row.get::<_, i64>(7)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Paged message list for a folder, newest server arrival first — full rows,
/// bodies included.
///
/// The UI does not use this: it pages with [`list_compact_by_folder_sorted`]
/// and loads one message at a time. This is the dump-everything shape the
/// `sync_test` example needs, so it stays deliberately simple.
pub fn list_by_folder(db: &Db, folder_id: i64, limit: u64, offset: u64) -> Result<Vec<Message>> {
    let order = folder_sort_clause("date", true);
    let mut stmt = db.conn().prepare(&format!(
        "select {COLS} from messages where folder_id = ?1
         order by {order} limit ?2 offset ?3"
    ))?;
    let rows = stmt
        .query_map(
            params![folder_id, limit as i64, offset as i64],
            row_to_message,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Fetch one message by id.
pub fn get(db: &Db, id: i64) -> Result<Message> {
    db.conn()
        .query_row(
            &format!("select {COLS} from messages where id = ?1"),
            [id],
            row_to_message,
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("message {id}")))
}

/// Fetch one message by `(folder_id, uid)` (used by sync/UI actions).
pub fn get_by_uid(db: &Db, folder_id: i64, uid: u32) -> Result<Message> {
    db.conn()
        .query_row(
            &format!("select {COLS} from messages where folder_id = ?1 and uid = ?2"),
            params![folder_id, uid as i64],
            row_to_message,
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("message uid {uid} in folder {folder_id}")))
}

/// Number of unread messages in a folder (badge counter).
pub fn count_unread(db: &Db, folder_id: i64) -> Result<u64> {
    let n: i64 = db.conn().query_row(
        "select count(*) from messages where folder_id = ?1 and is_read = 0",
        [folder_id],
        |r| r.get(0),
    )?;
    Ok(n as u64)
}

/// List UIDs of unread messages in a folder.
pub fn list_unread_uids(db: &Db, folder_id: i64) -> Result<Vec<u32>> {
    let mut stmt = db
        .conn()
        .prepare("select uid from messages where folder_id = ?1 and is_read = 0")?;
    let rows = stmt.query_map([folder_id], |r| r.get(0))?;
    let mut uids = Vec::new();
    for r in rows {
        uids.push(r?);
    }
    Ok(uids)
}

/// Total cached messages in a folder (drives the "load older" button:
/// shown rows vs cached rows vs server remainder).
pub fn count_by_folder(db: &Db, folder_id: i64) -> Result<u64> {
    let n: i64 = db.conn().query_row(
        "select count(*) from messages where folder_id = ?1",
        [folder_id],
        |r| r.get(0),
    )?;
    Ok(n as u64)
}

/// Cached and unread counts for one folder.
#[derive(Clone, Copy, Default)]
pub struct FolderCounts {
    pub total: u64,
    pub unread: u64,
}

/// Both counts for every folder of an account, in one pass.
///
/// The sidebar needs them for all folders at once, and asking per folder is
/// two queries each. Folders with no cached messages are absent from the
/// map -- callers treat a miss as zero.
pub fn counts_by_account(db: &Db, account_id: i64) -> Result<HashMap<i64, FolderCounts>> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "select folder_id, count(*), sum(case when is_read = 0 then 1 else 0 end)
         from messages
         where folder_id in (select id from folders where account_id = ?1)
         group by folder_id",
    )?;
    let rows = stmt.query_map([account_id], |r| {
        let folder_id: i64 = r.get(0)?;
        let total: i64 = r.get(1)?;
        let unread: i64 = r.get(2)?;
        Ok((
            folder_id,
            FolderCounts {
                total: total as u64,
                unread: unread as u64,
            },
        ))
    })?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, counts) = r?;
        out.insert(id, counts);
    }
    Ok(out)
}

/// Smallest cached UID in a folder, if any. Older-batch sync fetches server
/// UIDs below this; `None` means the folder is empty locally.
pub fn min_uid(db: &Db, folder_id: i64) -> Result<Option<u32>> {
    let v: Option<i64> = db.conn().query_row(
        "select min(uid) from messages where folder_id = ?1",
        [folder_id],
        |r| r.get(0),
    )?;
    Ok(v.map(|u| u as u32))
}

/// Delete a message (attachments cascade, FTS row removed by trigger).
pub fn delete(db: &Db, id: i64) -> Result<()> {
    let n = db
        .conn()
        .execute("delete from messages where id = ?1", [id])?;
    if n == 0 {
        return Err(StoreError::NotFound(format!("message {id}")));
    }
    Ok(())
}

/// All UIDs cached for a folder (for sync diffing).
pub fn list_uids(db: &Db, folder_id: i64) -> Result<Vec<u32>> {
    let mut stmt = db
        .conn()
        .prepare("select uid from messages where folder_id = ?1")?;
    let rows = stmt
        .query_map([folder_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().map(|u| u as u32).collect())
}

/// Delete all messages of a folder (UIDVALIDITY resync). Returns rows removed.
pub fn delete_by_folder(db: &Db, folder_id: i64) -> Result<u64> {
    let n = db
        .conn()
        .execute("delete from messages where folder_id = ?1", [folder_id])?;
    Ok(n as u64)
}

/// Delete one UID in a folder. Returns `true` if a row existed.
pub fn delete_by_uid(db: &Db, folder_id: i64, uid: u32) -> Result<bool> {
    let n = db.conn().execute(
        "delete from messages where folder_id = ?1 and uid = ?2",
        params![folder_id, uid as i64],
    )?;
    Ok(n > 0)
}

/// Delete a UID range `[lo, hi]` in one statement (QRESYNC VANISHED path).
/// Returns rows removed; never expands the range into individual UIDs.
pub fn delete_by_uid_range(db: &Db, folder_id: i64, lo: u32, hi: u32) -> Result<u64> {
    let (lo, hi) = (lo.min(hi) as i64, lo.max(hi) as i64);
    let n = db.conn().execute(
        "delete from messages where folder_id = ?1 and uid >= ?2 and uid <= ?3",
        params![folder_id, lo, hi],
    )?;
    Ok(n as u64)
}

/// Helper used by tests.
#[cfg(test)]
pub fn sample_new(account_id: i64, folder_id: i64, uid: u32) -> NewMessage {
    NewMessage {
        account_id,
        folder_id,
        uid,
        message_id_header: Some(format!("<{uid}@example.com>")),
        thread_id: None,
        subject: Some("Hello".to_string()),
        from_addr: Some("alice@example.com".to_string()),
        to_addrs: vec!["bob@example.com".to_string()],
        cc_addrs: vec![],
        bcc_addrs: vec![],
        reply_to: None,
        date: Some("2026-09-07T10:00:00+00:00".to_string()),
        snippet: Some("Hello Bob".to_string()),
        body_text: Some("Hello Bob, how are you?".to_string()),
        body_html: None,
        raw_headers: None,
        is_read: false,
        is_starred: false,
        is_draft: false,
        has_attachments: false,
        keywords: vec![],
        size: 128,
        downloaded_full: true,
    }
}

#[cfg(test)]
mod tests;
