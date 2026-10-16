//! Message-list reads, filters and list view-model words (step 0c).

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;

/// `MailNative.messageListed(folderId, uid)`: whether the message is still
/// one the lists show (`mailcore::store::messages::is_listed`); a reader
/// whose message is gone closes.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_messageListed<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> bool {
    unowned
        .with_env(|_env| -> Result<bool> {
            Ok(mailcore::store::messages::is_listed(
                crate::db::shared_db()?,
                folder_id,
                uid_arg(uid)?,
            )?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.emptyListText(searching, serverSearching, quickFilter,
/// unfiltered, query)`: what an empty message list says
/// (`mailcore::search::empty_list_text`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_emptyListText<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    searching: bool,
    server_searching: bool,
    quick_filter: bool,
    unfiltered: i32,
    query: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let text = mailcore::search::empty_list_text(
                searching,
                server_searching,
                quick_filter,
                unfiltered.max(0) as usize,
                &string(env, &query)?,
            );
            Ok(env.new_string(text)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.deletePrompt(confirmPref, bulk, permanentJson)`: whether a
/// delete destroys and whether to ask first — `{"permanent","ask"}`
/// (`mailcore::undo::delete_prompt`). `permanentJson` holds one target
/// folder's `delete_is_permanent` per entry, `null` for an unknown folder.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deletePrompt<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    confirm_pref: bool,
    bulk: bool,
    permanent_json: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let permanent: Vec<Option<bool>> =
                serde_json::from_str(&string(env, &permanent_json)?)?;
            let p = mailcore::undo::delete_prompt(confirm_pref, bulk, &permanent);
            Ok(env.new_string(serde_json::to_string(&p)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.markReadPlan(autoMarkRead, delaySecs, unread)`: whether and
/// when opening an unread row marks it read — `{"plan","delay_secs"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_markReadPlan<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    auto_mark_read: bool,
    delay_secs: i64,
    unread: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let (plan, delay) =
                match mailcore::store::settings::mark_read_plan(auto_mark_read, delay_secs, unread)
                {
                    mailcore::store::settings::MarkReadPlan::Off => ("off", 0),
                    mailcore::store::settings::MarkReadPlan::Now => ("now", 0),
                    mailcore::store::settings::MarkReadPlan::AfterDelay(s) => ("after", s),
                };
            let json = serde_json::json!({"plan": plan, "delay_secs": delay});
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.olderLabel(cached, server, filtered)`: the "Show older"
/// footer's words (`mailcore::feed::older_label`); `server < 0` = never
/// reported.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_olderLabel<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    cached: i64,
    server: i64,
    filtered: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let server = u64::try_from(server).ok();
            let label = mailcore::feed::older_label(cached.max(0) as u64, server, filtered);
            Ok(env.new_string(label)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.messagesJson(folderId, limit, offset)`: one list page in the
/// persisted sort order — `[{uid, subject, from, date, date_key, snippet,
/// unread, starred, has_attachments}]`, no bodies.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_messagesJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    limit: i64,
    offset: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::messages::messages_json(
                folder_id, limit, offset,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.messageRowsJson(folderId, uids)`: the list rows of `uids`
/// (a JSON array), shaped like `messagesJson`'s, in uid order
/// (`feed::message_rows_json`). The list swaps them in after a read/star
/// change instead of re-reading the folder.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_messageRowsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uids: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let uids = uids_json(&string(env, &uids)?)?;
            let rows =
                mailcore::feed::message_rows_json(crate::db::shared_db()?, folder_id, &uids)?;
            Ok(env.new_string(rows)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.listFilterKeep(filterJson, rowsJson)`: the list filters over
/// search hits (`list_filter::keep_json`) — a JSON array of the kept row
/// indexes. A folder's rows go through `listFilterUids` instead.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_listFilterKeep<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    filter_json: JString<'caller>,
    rows_json: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let kept = mailcore::search::list_filter::keep_json(
                &string(env, &filter_json)?,
                &string(env, &rows_json)?,
            )?;
            Ok(env.new_string(kept)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.listFilterUids(folderId, filterJson)`: the list filters over
/// a folder's rows, read from the cache (`list_filter::keep_in_folder`) — a
/// JSON array of the kept uids. No rows go in.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_listFilterUids<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    filter_json: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let kept = mailcore::search::list_filter::keep_in_folder_json(
                crate::db::shared_db()?,
                folder_id,
                &string(env, &filter_json)?,
            )?;
            Ok(env.new_string(kept)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
