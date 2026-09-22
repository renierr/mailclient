//! Message reads and local flag writes.
//!
//! Flag writes never touch the network: the change lands in SQLite with
//! `flags_dirty` set and a background push carries it to the server seconds
//! later. Clicking a message must not be able to block on IMAP, and it must
//! not be lost by quitting straight afterwards either — which is what the
//! [`crate::net::spawn_flag_push`] each of these ends with buys.

use mailcore::store::messages;

use crate::db::shared_db;
use crate::net::spawn_flag_push;

/// One page of the message list as JSON: `[{uid, subject, from, date,
/// date_key, snippet, unread, starred, has_attachments}]`, in the order the
/// persisted sort setting asks for.
///
/// Compact on purpose — bodies are not sanitized here. The reader asks for
/// [`message_json`] when a row is actually selected, so opening a folder does
/// not process 200 message bodies.
pub fn messages_json(folder_id: i64, limit: i64, offset: i64) -> anyhow::Result<String> {
    Ok(mailcore::feed::messages_list_json_paged(
        shared_db()?,
        folder_id,
        limit.max(0) as u64,
        offset.max(0) as u64,
    )?)
}

/// Everything the reader needs for one message, bodies sanitized.
pub fn message_json(folder_id: i64, uid: u32) -> anyhow::Result<String> {
    Ok(mailcore::feed::message_json(shared_db()?, folder_id, uid)?)
}

/// Sanitized HTML for one message.
///
/// `allow_remote` re-sanitizes the stored body with remote images kept — the
/// "Show once" path. The list feed strips them when the setting is off, so it
/// cannot simply reuse what it already has.
pub fn message_html(folder_id: i64, uid: u32, allow_remote: bool) -> anyhow::Result<String> {
    Ok(mailcore::feed::message_html(
        shared_db()?,
        folder_id,
        uid,
        allow_remote,
    )?)
}

/// Attachment metadata for one message (no bytes).
pub fn attachments_json(folder_id: i64, uid: u32) -> anyhow::Result<String> {
    Ok(mailcore::feed::attachments_json(
        shared_db()?,
        folder_id,
        uid,
    )?)
}

/// `{from, to, cc, date, subject, message_id, reply_to}` for the headers view.
pub fn headers_json(folder_id: i64, uid: u32) -> anyhow::Result<String> {
    Ok(mailcore::feed::headers_json(shared_db()?, folder_id, uid)?)
}

/// Set the read flag on one message.
pub fn mark_read(account_id: i64, folder_id: i64, uid: u32, read: bool) -> anyhow::Result<()> {
    mark_read_many(account_id, folder_id, vec![uid], read).map(|_| ())
}

/// Set the read flag on a selection.
pub fn mark_read_many(
    account_id: i64,
    folder_id: i64,
    uids: Vec<u32>,
    read: bool,
) -> anyhow::Result<u64> {
    let db = shared_db()?;
    let n = messages::set_read_many_by_uids(db, folder_id, &uids, read)?;
    spawn_flag_push(account_id);
    Ok(n)
}

/// Set the starred flag on a selection.
pub fn set_star_many(
    account_id: i64,
    folder_id: i64,
    uids: Vec<u32>,
    starred: bool,
) -> anyhow::Result<u64> {
    let db = shared_db()?;
    let n = messages::set_star_many_by_uids(db, folder_id, &uids, starred)?;
    spawn_flag_push(account_id);
    Ok(n)
}

/// Flip one message's starred flag and report the new state.
pub fn toggle_star(account_id: i64, folder_id: i64, uid: u32) -> anyhow::Result<bool> {
    let db = shared_db()?;
    let now_starred = !messages::get_by_uid(db, folder_id, uid)?.is_starred;
    messages::set_star_many_by_uids(db, folder_id, &[uid], now_starred)?;
    spawn_flag_push(account_id);
    Ok(now_starred)
}
