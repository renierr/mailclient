//! Attachment bytes, downloads and staging dirs.

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JByteArray, JClass, JString};
use jni::EnvUnowned;

/// `MailNative.cachedAttachmentBytes(attachmentId)`: cached bytes, or an
/// error when they are not downloaded yet — then queue `downloadAttachments`
/// and await its `Attachments` finished event. Bytes cross as a `byte[]`,
/// like the FRB `Vec<u8>`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_cachedAttachmentBytes<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    attachment_id: i64,
) -> JByteArray<'caller> {
    unowned
        .with_env(|env| -> Result<JByteArray<'caller>> {
            match crate::api::attachments::cached_attachment_bytes(attachment_id)? {
                Some(bytes) => Ok(env.byte_array_from_slice(&bytes)?),
                None => Err(BridgeError("not downloaded yet".to_string())),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.downloadMessageFiles(accountId, folderId, uid)`: fetch every
/// attachment of one message into the local cache, blocking the calling
/// worker thread (whole-message: IMAP fetches by body part within one
/// FETCH). Returns how many files landed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_downloadMessageFiles<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let db = crate::db::shared_db()?;
            let m = mailcore::store::messages::get_by_uid(db, folder_id, uid_arg(uid)?)?;
            if m.account_id != account_id {
                return Err(BridgeError(
                    "message does not belong to this account".to_string(),
                ));
            }
            let files: u64 = blocking(async {
                mailcore::sync::attachments::download(db, m.id)
                    .await
                    .map_err(anyhow::Error::msg)
            })?;
            Ok(env.new_string(files.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.downloadAttachments(accountId, folderId, uid)`: queue fetching
/// every attachment into the cache; the bytes land with the `Attachments`
/// finished event. Same job as the FRB `download_attachments` (same
/// `attach:…` dedupe key), so a tap while a download runs waits instead of
/// stacking. Prefer this over `downloadMessageFiles`: the job runs on the
/// net thread, while the blocking form drives a second IMAP session off it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_downloadAttachments<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::attachments::download_attachments(account_id, folder_id, uid_arg(uid)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.attachmentsPending(folderId, uid)`: whether that message's
/// attachment download is still queued or running. Downloads of different
/// messages share the `Attachments` job kind, so a finish event alone does
/// not say whose it was.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_attachmentsPending<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> bool {
    unowned
        .with_env(|_env| -> Result<bool> {
            Ok(crate::net::is_inflight(&crate::net::attachments_key(
                folder_id,
                uid_arg(uid)?,
            )))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.attachmentOpenMime(attachmentId)`: the opener MIME for the
/// stored row, same derivation as the feed's `open_mime`. Re-read this
/// after a download rather than reusing the reader payload's copy: the
/// download-time magic check may have corrected the stored header since
/// the message was read, and the old MIME would send the file to the
/// wrong app.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_attachmentOpenMime<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    attachment_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let a =
                mailcore::store::messages::get_attachment(crate::db::shared_db()?, attachment_id)?;
            let file_name = mailcore::paths::safe_attachment_name_for_mime(
                a.filename.as_deref(),
                a.mime_type.as_deref(),
                a.id,
            );
            Ok(env.new_string(mailcore::mime::open_mime(
                a.mime_type.as_deref(),
                Some(&file_name),
            ))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.writeAttachmentCopy(attachmentId, dir)`: the copy a system
/// viewer opens, under a name that cannot clash or escape `dir`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_writeAttachmentCopy<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    attachment_id: i64,
    dir: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let path =
                crate::api::attachments::write_attachment_copy(attachment_id, string(env, &dir)?)?;
            Ok(env.new_string(path)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.pickStageDir(base)`: a fresh staging dir under `base` for a
/// file attached in the composer (stale ones pruned first) — its path.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_pickStageDir<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    base: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let base = string(env, &base)?;
            let dir = mailcore::paths::pick_stage_dir(std::path::Path::new(&base))
                .map_err(anyhow::Error::from)?;
            Ok(env.new_string(dir.to_string_lossy())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
