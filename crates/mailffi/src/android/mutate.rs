//! Flag writes, moves, purges and undo (step 0c).

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::{Env, EnvUnowned};

/// `MailNative.setReadFlag(accountId, folderId, uid, read)`: local flag write
/// with background push, like `api::messages::mark_read`. (Named apart from
/// the notification `markRead`, which marks a `ReadTarget`.)
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setReadFlag<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
    read: bool,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::messages::mark_read(account_id, folder_id, uid_arg(uid)?, read)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.toggleStar(accountId, folderId, uid)`: flip one message's
/// starred flag, like `api::messages::toggle_star`. The reader re-reads the
/// message for the new state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_toggleStar<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::messages::toggle_star(account_id, folder_id, uid_arg(uid)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

fn move_result_json<'a>(
    env: &mut Env<'a>,
    r: crate::api::mutate::MoveResult,
) -> Result<JString<'a>> {
    let json = serde_json::json!({
        "batch": r.batch,
        "label": r.label,
        "purging": r.purging,
    });
    Ok(env.new_string(serde_json::to_string(&json)?)?)
}

/// `MailNative.deleteMessage(accountId, folderId, uid)`: Trash (undoable)
/// or a purge job where Trash does not apply — `{"batch","label","purging"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r =
                crate::api::mutate::delete_messages(account_id, folder_id, vec![uid_arg(uid)?])?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.archiveMessage(accountId, folderId, uid)`: one-click archive.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_archiveMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r =
                crate::api::mutate::archive_messages(account_id, folder_id, vec![uid_arg(uid)?])?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.moveMessage(accountId, folderId, uid, destPath)`: move to any
/// folder of the same account, addressed by path.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_moveMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
    dest_path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::move_messages(
                account_id,
                folder_id,
                vec![uid_arg(uid)?],
                string(env, &dest_path)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.purgeMessage(accountId, folderId, uid)`: destroy server-side.
/// No undo — the UI confirms first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_purgeMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::mutate::purge_messages(account_id, folder_id, vec![uid_arg(uid)?])?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.undoMove(batch)`: take back a queued action; the status line
/// text, also when it was too late.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_undoMove<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    batch: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let text = crate::api::mutate::undo_move(string(env, &batch)?)?;
            Ok(env.new_string(text)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.undoGraceSecs()`: seconds an action stays undoable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_undoGraceSecs<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::mutate::undo_grace_secs().to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.markReadMany(accountId, folderId, uids, read)`: local flag
/// write + background push; how many rows changed, back as a string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_markReadMany<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
    read: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = crate::api::messages::mark_read_many(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
                read,
            )?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setStarMany(accountId, folderId, uids, starred)`: same for
/// the starred flag.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setStarMany<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
    starred: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = crate::api::messages::set_star_many(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
                starred,
            )?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.markReadHits(accountId, hits, read)`: the read flag over
/// cross-folder search hits.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_markReadHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
    read: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = crate::api::messages::mark_read_hits(
                account_id,
                hits_json(&string(env, &hits)?)?,
                read,
            )?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setStarHits(accountId, hits, starred)`: the starred flag over
/// cross-folder search hits.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setStarHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
    starred: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = crate::api::messages::set_star_hits(
                account_id,
                hits_json(&string(env, &hits)?)?,
                starred,
            )?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.deleteMessages(accountId, folderId, uids)`: Trash (undoable)
/// or purge where Trash does not apply — `{"batch","label","purging"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::delete_messages(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.archiveMessages(accountId, folderId, uids)`: one-click
/// archive of a selection.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_archiveMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::archive_messages(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.moveMessages(accountId, folderId, uids, destPath)`: a
/// selection to any folder of the same account, addressed by path.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_moveMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
    dest_path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::move_messages(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
                string(env, &dest_path)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.purgeMessages(accountId, folderId, uids)`: destroy
/// server-side. No undo — the UI confirms first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_purgeMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::mutate::purge_messages(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
            )?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.deleteHits(accountId, hits)`: search hits across folders — one
/// Undo for what goes to Trash, one purge job for the shares that destroy
/// (the UI confirmed those first).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::delete_hits(account_id, hits_json(&string(env, &hits)?)?)?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.archiveHits(accountId, hits)`: hits across folders, one Undo.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_archiveHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::archive_hits(account_id, hits_json(&string(env, &hits)?)?)?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.moveHits(accountId, hits, destPath)`: hits across folders to
/// `destPath`, one Undo.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_moveHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
    dest_path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::move_hits(
                account_id,
                hits_json(&string(env, &hits)?)?,
                string(env, &dest_path)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.purgeHits(accountId, hits)`: destroy hits across folders in
/// one job. No undo — the UI confirms first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_purgeHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::mutate::purge_hits(account_id, hits_json(&string(env, &hits)?)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.createFolder(accountId, path)`: queued IMAP folder creation;
/// a `Folders` finished event says when. `/` separates levels, missing
/// parents are created, an existing path is success.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_createFolder<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    path: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::mutate::create_folder(account_id, string(env, &path)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
