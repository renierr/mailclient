//! CRUD for `messages` + `attachments` metadata.

use rusqlite::{params, OptionalExtension};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Attachment, Message, NewAttachment, NewMessage};
use crate::store::{json_vec, now, opt_bool};

fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let to: String = row.get(8)?;
    let cc: String = row.get(9)?;
    let bcc: String = row.get(10)?;
    let kw: String = row.get(17)?;
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
        is_read: opt_bool(row.get::<_, i64>(16)?),
        is_starred: opt_bool(row.get::<_, i64>(18)?),
        is_draft: opt_bool(row.get::<_, i64>(19)?),
        has_attachments: opt_bool(row.get::<_, i64>(20)?),
        keywords: json_vec(&kw).unwrap_or_default(),
        size: row.get::<_, i64>(21)? as u64,
        downloaded_full: opt_bool(row.get::<_, i64>(22)?),
    })
}

// Column order must match row_to_message indices.
const COLS: &str = "id, account_id, folder_id, uid, message_id_header, thread_id,
    subject, from_addr, to_addrs, cc_addrs, bcc_addrs, reply_to, date, snippet,
    body_text, body_html, is_read, keywords, is_starred, is_draft,
    has_attachments, size, downloaded_full";

/// Insert a message, or replace it if the same `(account, folder, uid)` exists.
pub fn upsert(db: &Db, m: &NewMessage) -> Result<i64> {
    let ts = now();
    db.conn().execute(
        "insert into messages (account_id, folder_id, uid, message_id_header,
            thread_id, subject, from_addr, to_addrs, cc_addrs, bcc_addrs,
            reply_to, date, snippet, body_text, body_html, is_read, keywords,
            is_starred, is_draft, has_attachments, size, downloaded_full,
            created_at, updated_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?23)
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
            is_read = excluded.is_read,
            keywords = excluded.keywords,
            is_starred = excluded.is_starred,
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

/// Paged message list for a folder, newest first.
pub fn list_by_folder(db: &Db, folder_id: i64, limit: u64, offset: u64) -> Result<Vec<Message>> {
    let mut stmt = db.conn().prepare(&format!(
        "select {COLS} from messages where folder_id = ?1
         order by date desc, id desc limit ?2 offset ?3"
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

/// Mark one message's flags as pushed.
pub fn clear_flags_dirty(db: &Db, id: i64) -> Result<()> {
    db.conn()
        .execute("update messages set flags_dirty = 0 where id = ?1", [id])?;
    Ok(())
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

/// Record one attachment. `size` is stored as given so metadata-only rows
/// (`data=None`) still report real sizes; full rows should pass
/// `size == data.len()`.
pub fn add_attachment(db: &Db, message_id: i64, a: &NewAttachment) -> Result<i64> {
    db.conn().execute(
        "insert into attachments (message_id, filename, mime_type, size,
            content_id, storage_path, data, is_inline, created_at)
         values (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8)",
        params![
            message_id,
            a.filename,
            a.mime_type,
            a.size as i64,
            a.content_id,
            a.data.as_deref(),
            i64::from(a.is_inline),
            now()
        ],
    )?;
    Ok(db.conn().last_insert_rowid())
}

/// List attachment metadata of a message (no BLOB bytes — keeps feeds cheap).
/// Ordered with regular attachments first, inline parts last.
pub fn list_attachments(db: &Db, message_id: i64) -> Result<Vec<Attachment>> {
    let mut stmt = db.conn().prepare(
        "select id, message_id, filename, mime_type, size, content_id,
            storage_path, is_inline
         from attachments where message_id = ?1 order by is_inline, id",
    )?;
    let rows = stmt
        .query_map([message_id], |row| {
            Ok(Attachment {
                id: row.get(0)?,
                message_id: row.get(1)?,
                filename: row.get(2)?,
                mime_type: row.get(3)?,
                size: row.get::<_, i64>(4)? as u64,
                content_id: row.get(5)?,
                storage_path: row.get(6)?,
                data: None,
                is_inline: row.get::<_, i64>(7)? != 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Fetch one attachment with its bytes (for save/open).
pub fn get_attachment(db: &Db, id: i64) -> Result<Attachment> {
    db.conn()
        .query_row(
            "select id, message_id, filename, mime_type, size, content_id,
                storage_path, data, is_inline
             from attachments where id = ?1",
            [id],
            |row| {
                Ok(Attachment {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    filename: row.get(2)?,
                    mime_type: row.get(3)?,
                    size: row.get::<_, i64>(4)? as u64,
                    content_id: row.get(5)?,
                    storage_path: row.get(6)?,
                    data: row.get::<_, Option<Vec<u8>>>(7)?,
                    is_inline: row.get::<_, i64>(8)? != 0,
                })
            },
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("attachment {id}")))
}

/// Whether one attachment already has bytes (inline BLOB or legacy file).
/// Drives on-demand downloads: `false` means the next save must fetch first.
pub fn attachment_has_data(db: &Db, id: i64) -> Result<bool> {
    let n: i64 = db.conn().query_row(
        "select case when (data is not null and length(data) > 0)
            or storage_path is not null then 1 else 0 end
         from attachments where id = ?1",
        [id],
        |row| row.get(0),
    )?;
    Ok(n != 0)
}

/// Refresh only the `has_attachments` flag (used after an on-demand
/// attachment fetch; unlike `upsert` this never touches read/star state).
pub fn set_has_attachments(db: &Db, id: i64, has: bool) -> Result<()> {
    db.conn().execute(
        "update messages set has_attachments = ?1, updated_at = ?2 where id = ?3",
        params![i64::from(has), now(), id],
    )?;
    Ok(())
}

/// Write one attachment's bytes to `dest_path`. Falls back to the legacy
/// `storage_path` file when the row has no inline blob.
pub fn save_attachment_to_path(db: &Db, id: i64, dest_path: &std::path::Path) -> Result<u64> {
    let a = get_attachment(db, id)?;
    if let Some(bytes) = a.data.filter(|b| !b.is_empty()) {
        std::fs::write(dest_path, &bytes)?;
        Ok(bytes.len() as u64)
    } else if let Some(src) = a.storage_path {
        let n = std::fs::copy(&src, dest_path)?;
        Ok(n)
    } else {
        Err(StoreError::NotFound(format!(
            "attachment {id} has no stored data"
        )))
    }
}

/// Delete all attachments of a message (used before re-storing on resync).
pub fn delete_attachments_for_message(db: &Db, message_id: i64) -> Result<()> {
    db.conn().execute(
        "delete from attachments where message_id = ?1",
        [message_id],
    )?;
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

/// Update flags of one UID in a folder (no-op if unknown).
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
         where account_id = ?5 and folder_id = ?6 and uid = ?7",
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
mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount};
    use crate::store::{accounts, folders};

    fn setup() -> (Db, i64, i64) {
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
        let f = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        (db, acc, f)
    }

    #[test]
    fn upsert_list_flags_unread_delete() {
        let (db, acc, f) = setup();
        let id = upsert(&db, &sample_new(acc, f, 1)).unwrap();
        // Same UID upserts to the same row.
        assert_eq!(upsert(&db, &sample_new(acc, f, 1)).unwrap(), id);
        upsert(&db, &sample_new(acc, f, 2)).unwrap();
        assert_eq!(list_by_folder(&db, f, 10, 0).unwrap().len(), 2);
        assert_eq!(count_unread(&db, f).unwrap(), 2);
        set_flags(&db, id, true, true).unwrap();
        let m = get(&db, id).unwrap();
        assert!(m.is_read && m.is_starred);
        assert_eq!(m.to_addrs, vec!["bob@example.com".to_string()]);
        assert_eq!(count_unread(&db, f).unwrap(), 1);
        delete(&db, id).unwrap();
        assert!(matches!(get(&db, id), Err(StoreError::NotFound(_))));
    }

    #[test]
    fn local_flag_change_queues_for_push_then_clears() {
        let (db, acc, f) = setup();
        let id = upsert(&db, &sample_new(acc, f, 1)).unwrap();
        let other = upsert(&db, &sample_new(acc, f, 2)).unwrap();
        // A freshly synced message owes the server nothing.
        assert!(list_flags_dirty(&db, acc).unwrap().is_empty());

        // Reading a message locally (the click path) queues the flag push
        // instead of doing it inline.
        set_flags(&db, id, true, false).unwrap();
        let dirty = list_flags_dirty(&db, acc).unwrap();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].id, id);
        assert!(dirty[0].is_read);

        // Once pushed, the row is settled and stays out of the queue.
        clear_flags_dirty(&db, id).unwrap();
        assert!(list_flags_dirty(&db, acc).unwrap().is_empty());

        // Starring queues too, and only the touched row.
        set_flags(&db, other, false, true).unwrap();
        let dirty = list_flags_dirty(&db, acc).unwrap();
        assert_eq!(dirty.len(), 1);
        assert_eq!(dirty[0].id, other);
        assert!(dirty[0].is_starred);
    }

    #[test]
    fn count_and_min_uid_track_cache() {
        let (db, acc, f) = setup();
        assert_eq!(count_by_folder(&db, f).unwrap(), 0);
        assert_eq!(min_uid(&db, f).unwrap(), None);
        upsert(&db, &sample_new(acc, f, 5)).unwrap();
        upsert(&db, &sample_new(acc, f, 9)).unwrap();
        assert_eq!(count_by_folder(&db, f).unwrap(), 2);
        assert_eq!(min_uid(&db, f).unwrap(), Some(5));
    }

    #[test]
    fn json_vec_helper() {
        assert_eq!(json_vec("[]").unwrap(), Vec::<String>::new());
        assert_eq!(json_vec("").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn attachment_blob_roundtrips_and_saves_to_disk() {
        use crate::models::NewAttachment;
        let (db, acc, f) = setup();
        let id = upsert(&db, &sample_new(acc, f, 60)).unwrap();
        // Metadata-only listing carries no bytes (feed stays cheap).
        let aid = add_attachment(
            &db,
            id,
            &NewAttachment {
                filename: Some("notes.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                content_id: None,
                size: 16,
                data: Some(b"hello attachment".to_vec()),
                is_inline: false,
            },
        )
        .unwrap();
        let listed = list_attachments(&db, id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].filename.as_deref(), Some("notes.txt"));
        assert_eq!(listed[0].size, 16);
        assert!(listed[0].data.is_none());
        assert!(!listed[0].is_inline);
        assert!(attachment_has_data(&db, aid).unwrap());
        // Full fetch carries the bytes; save writes them back to disk.
        let full = get_attachment(&db, aid).unwrap();
        assert_eq!(full.data.as_deref(), Some(b"hello attachment".as_slice()));
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.txt");
        let n = save_attachment_to_path(&db, aid, &dest).unwrap();
        assert_eq!(n, 16);
        assert_eq!(std::fs::read(&dest).unwrap(), b"hello attachment");
        // Resync replacement clears stale files.
        delete_attachments_for_message(&db, id).unwrap();
        assert!(list_attachments(&db, id).unwrap().is_empty());
    }

    #[test]
    fn attachment_meta_row_reports_no_data() {
        use crate::models::NewAttachment;
        let (db, acc, f) = setup();
        let id = upsert(&db, &sample_new(acc, f, 61)).unwrap();
        // What background sync stores: real name/size, no bytes.
        let aid = add_attachment(
            &db,
            id,
            &NewAttachment {
                filename: Some("doc.pdf".to_string()),
                mime_type: Some("application/pdf".to_string()),
                content_id: None,
                size: 11,
                data: None,
                is_inline: false,
            },
        )
        .unwrap();
        assert!(!attachment_has_data(&db, aid).unwrap());
        let listed = list_attachments(&db, id).unwrap();
        assert_eq!(listed[0].size, 11);
        // Saving without bytes is a clean NotFound (the bridge downloads first).
        let dir = tempfile::tempdir().unwrap();
        assert!(save_attachment_to_path(&db, aid, &dir.path().join("x")).is_err());
        // Only the flag flips on refresh — never read/star state.
        set_has_attachments(&db, id, true).unwrap();
        assert!(get(&db, id).unwrap().has_attachments);
    }
}
