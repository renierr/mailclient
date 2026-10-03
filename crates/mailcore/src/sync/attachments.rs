//! On-demand attachment download: the one place file bytes cross the network.
//!
//! Background sync stores names and sizes only; bytes are fetched when the
//! user asks for them (open, save, forward, export) and then cached in SQLite.
//! Both adapters call this instead of driving the IMAP session themselves.

use crate::db::Db;
use crate::store::{accounts, folders, messages};

use super::pool::checkout_session;

/// Make sure a message's file bytes are cached, downloading them now when
/// any are missing. Returns the number of files stored (0 = already
/// cached). `include_inline` also requires inline parts (draft reopening and
/// .eml export keep them; open/save only care about real attachments).
pub async fn ensure_cached(db: &Db, message_id: i64, include_inline: bool) -> Result<u64, String> {
    if missing_count(db, message_id, include_inline).map_err(|e| e.to_string())? == 0 {
        return Ok(0);
    }
    download(db, message_id).await
}

/// Download every attachment of one message, cached or not. Returns the
/// number of files stored.
pub async fn download(db: &Db, message_id: i64) -> Result<u64, String> {
    let msg = messages::get(db, message_id).map_err(|e| e.to_string())?;
    let folder = folders::get(db, msg.folder_id).map_err(|e| e.to_string())?;
    let acc = accounts::get(db, folder.account_id).map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    let mut imap = checkout_session(&acc).await?;
    let result = imap.fetch_attachments(db, message_id).await;
    imap.checkin();
    let n = result.map_err(|e| e.to_string())?;
    log::info!(
        "attachments: stored {n} file(s) for message {message_id} in {:?}",
        started.elapsed()
    );
    Ok(n)
}

/// How many of a message's files have no cached bytes.
pub fn missing_count(
    db: &Db,
    message_id: i64,
    include_inline: bool,
) -> crate::error::Result<usize> {
    let mut n = 0;
    for a in messages::list_attachments(db, message_id)?
        .iter()
        .filter(|a| include_inline || !a.is_inline)
    {
        if !messages::attachment_has_data(db, a.id)? {
            n += 1;
        }
    }
    Ok(n)
}
