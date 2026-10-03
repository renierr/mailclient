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

/// Whether and when opening an `unread` row marks it read, from the two
/// settings and the row state (`store::settings::mark_read_plan`).
#[flutter_rust_bridge::frb(sync)]
pub fn mark_read_plan(auto_mark_read: bool, delay_secs: i64, unread: bool) -> MarkReadPlan {
    let (plan, delay_secs) =
        match mailcore::store::settings::mark_read_plan(auto_mark_read, delay_secs, unread) {
            mailcore::store::settings::MarkReadPlan::Off => ("off", 0),
            mailcore::store::settings::MarkReadPlan::Now => ("now", 0),
            mailcore::store::settings::MarkReadPlan::AfterDelay(s) => ("after", s),
        };
    MarkReadPlan {
        plan: plan.to_string(),
        delay_secs,
    }
}

/// [`mark_read_plan`] as a generated struct.
pub struct MarkReadPlan {
    /// `"off"`, `"now"` or `"after"`.
    pub plan: String,
    /// Seconds to wait when `plan` is `"after"`, else `0`.
    pub delay_secs: u64,
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

/// Set the read flag on search hits across folders (`mailcore::bulk`).
pub fn mark_read_hits(
    account_id: i64,
    hits: Vec<crate::api::mutate::Hit>,
    read: bool,
) -> anyhow::Result<u64> {
    let db = shared_db()?;
    let groups = crate::api::mutate::groups(db, account_id, hits)?;
    let n = mailcore::bulk::set_read(db, &groups, read)?;
    spawn_flag_push(account_id);
    Ok(n)
}

/// Set the starred flag on search hits across folders.
pub fn set_star_hits(
    account_id: i64,
    hits: Vec<crate::api::mutate::Hit>,
    starred: bool,
) -> anyhow::Result<u64> {
    let db = shared_db()?;
    let groups = crate::api::mutate::groups(db, account_id, hits)?;
    let n = mailcore::bulk::set_starred(db, &groups, starred)?;
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

/// A clicked link split for the examine dialog, and whether it may be opened
/// at all (`mailcore::html::link_info`, the sanitizer's own rule).
#[flutter_rust_bridge::frb(sync)]
pub fn link_info(url: String) -> LinkInfo {
    let i = mailcore::html::link_info(&url);
    LinkInfo {
        safe: i.safe,
        scheme: i.scheme,
        host: i.host,
        path: i.path,
    }
}

/// [`mailcore::html::LinkInfo`] as a generated struct; `""` = none.
pub struct LinkInfo {
    /// `http`, `https` or `mailto`: may be opened (user-gated).
    pub safe: bool,
    pub scheme: String,
    pub host: String,
    pub path: String,
}

/// Export one message as standard RFC 5322 .eml bytes.
pub fn export_message_eml_bytes(folder_id: i64, uid: u32) -> anyhow::Result<Vec<u8>> {
    let db = shared_db()?;
    let bytes = mailcore::export::assemble_eml(db, folder_id, uid)?;
    Ok(bytes)
}

/// Export one message as .eml to a file path.
pub fn export_message_eml(folder_id: i64, uid: u32, path: String) -> anyhow::Result<String> {
    let db = shared_db()?;
    let dest = mailcore::export::export_eml_to(db, folder_id, uid, &path)?;
    Ok(dest.to_string_lossy().into_owned())
}

/// Suggested filename for exporting a message as .eml.
#[flutter_rust_bridge::frb(sync)]
pub fn suggested_eml_name(folder_id: i64, uid: u32) -> String {
    let Ok(db) = shared_db() else {
        return format!("message-{uid}.eml");
    };
    mailcore::export::suggested_eml_name(db, folder_id, uid)
}
