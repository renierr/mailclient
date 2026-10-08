//! The `attachments` table: metadata rows, their bytes, and getting one
//! back out to disk.
//!
//! Bytes live in the BLOB column so a message and its files travel with the
//! single SQLite file; sync stores names and sizes only, and the bytes
//! arrive on an explicit user request. The exception is inline images
//! (`cid:` parts), whose bytes sync keeps so the reader can show a body
//! without going online.

use rusqlite::{params, OptionalExtension};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Attachment, NewAttachment};
use crate::store::now;

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

/// Replace a message's attachments with a freshly parsed set while keeping
/// stable row IDs: matches on filename/content-id/size/inline, fills
/// bytes in place, inserts truly new parts, deletes vanished ones — all in
/// one transaction so an open/save click holding a pre-download ID still
/// resolves afterwards (`unchecked_` because `Db::conn()` is shared `&`).
///
/// The MIME type is deliberately not part of the match: the download-time
/// magic check (`mime::corrected_mime`) may fix a header the sync-time
/// metadata row got wrong, and that correction must update the row in
/// place rather than orphan the ID the reader holds.
pub fn replace_attachments(db: &Db, message_id: i64, files: &[NewAttachment]) -> Result<()> {
    // `Immediate`, not the deferred `unchecked_transaction()`: a deferred
    // `BEGIN` followed by a read holds a snapshot the first write has to
    // upgrade, and a commit from another connection in between (the GUI and
    // net threads each have one, and the `--sync-once` CLI shares the file)
    // fails that upgrade with SQLITE_BUSY_SNAPSHOT, which the busy handler
    // does not retry. Taking the write lock up front makes a second writer
    // queue instead.
    //
    // The read stays *inside*: done before the `BEGIN`, two writers for the
    // same message could both see the old rows and insert duplicates, or
    // update a row the other had just deleted.
    let tx =
        rusqlite::Transaction::new_unchecked(db.conn(), rusqlite::TransactionBehavior::Immediate)?;
    let mut existing = list_attachments(db, message_id)?;
    for file in files {
        let matched = existing.iter().position(|a| {
            a.filename == file.filename
                && a.content_id == file.content_id
                && a.size == file.size
                && a.is_inline == file.is_inline
        });
        if let Some(index) = matched {
            let attachment = existing.remove(index);
            tx.execute(
                "update attachments set data = coalesce(?1, data),
                    mime_type = coalesce(?2, mime_type),
                    storage_path = case when ?1 is not null then null else storage_path end
                 where id = ?3",
                params![file.data.as_deref(), file.mime_type, attachment.id],
            )?;
        } else {
            tx.execute(
                "insert into attachments (message_id, filename, mime_type, size,
                    content_id, storage_path, data, is_inline, created_at)
                 values (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8)",
                params![
                    message_id,
                    file.filename,
                    file.mime_type,
                    file.size as i64,
                    file.content_id,
                    file.data.as_deref(),
                    i64::from(file.is_inline),
                    now()
                ],
            )?;
        }
    }
    for attachment in existing {
        tx.execute("delete from attachments where id = ?1", [attachment.id])?;
    }
    tx.commit()?;
    Ok(())
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

/// Stored bytes of every `cid:`-addressable image part of a message, for
/// [`crate::html::inline_cid_images`]. Local only: parts whose bytes were
/// never stored are simply absent.
pub fn inline_images(db: &Db, message_id: i64) -> Result<Vec<crate::html::InlineImage>> {
    let mut stmt = db.conn().prepare(
        "select content_id, mime_type, data from attachments
         where message_id = ?1 and content_id is not null
           and data is not null and length(data) between 1 and ?2
           and lower(mime_type) like 'image/%'
         order by id",
    )?;
    let rows = stmt
        .query_map(
            params![message_id, crate::html::MAX_INLINE_IMAGE_BYTES as i64],
            |row| {
                Ok(crate::html::InlineImage {
                    content_id: row.get(0)?,
                    mime_type: row.get(1)?,
                    data: row.get(2)?,
                })
            },
        )?
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
    // Optional, not a bare `query_row`: with no row the raw
    // `Query returned no rows` used to reach the caller, so a missing
    // attachment showed up in the reader as a database error instead of
    // "attachment not found".
    let n: Option<i64> = db
        .conn()
        .query_row(
            "select case when (data is not null and length(data) > 0)
                or storage_path is not null then 1 else 0 end
             from attachments where id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    // `Err`, not `Ok(false)`: `false` tells a caller the bytes are not cached
    // yet, and it would start a download for an attachment that does not exist.
    n.map(|v| v != 0)
        .ok_or_else(|| StoreError::NotFound(format!("attachment {id}")))
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
