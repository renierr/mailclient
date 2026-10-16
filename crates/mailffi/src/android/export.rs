//! .eml export: fetch missing files, then assemble locally.

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JByteArray, JClass, JString};
use jni::EnvUnowned;

/// `MailNative.suggestedEmlName(folderId, uid)`: filesystem-safe `.eml` name.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_suggestedEmlName<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let name = mailcore::export::suggested_eml_name(
                crate::db::shared_db()?,
                folder_id,
                uid_arg(uid)?,
            );
            Ok(env.new_string(name)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.exportFetch(folderId, uid)`: the "Export" job — download
/// the files the message still lacks, like Qt's `export_message`. Queued on
/// the net thread under `export:{folder}:{uid}`; once it finishes,
/// `exportEmlBytes` assembles the file without touching the network.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_exportFetch<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            let uid = uid_arg(uid)?;
            crate::net::spawn(
                "Export",
                format!("export:{folder_id}:{uid}"),
                move |db, progress| async move {
                    if mailcore::export::missing(db, folder_id, uid).unwrap_or(0) > 0 {
                        progress.report("Downloading…");
                    }
                    mailcore::export::prepare(db, folder_id, uid).await?;
                    Ok(("Message ready to save".to_string(), None))
                },
            )
            .map_err(BridgeError::from)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.assembleEml(folderId, uid)`: the cached message as `.eml`
/// bytes, local only. Fails while attachment bytes are missing, so
/// `exportFetch` runs first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_assembleEml<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JByteArray<'caller> {
    unowned
        .with_env(|env| -> Result<JByteArray<'caller>> {
            let eml =
                mailcore::export::assemble_eml(crate::db::shared_db()?, folder_id, uid_arg(uid)?)?;
            Ok(env.byte_array_from_slice(&eml)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.exportEmlBytes(folderId, uid)`: download-then-assemble as one
/// blocking call, off the job queue. Only the retired Flutter app's
/// `ReaderActivity` calls it; the native app uses `exportFetch` +
/// `assembleEml`. Fails while attachment bytes are missing and
/// undownloadable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_exportEmlBytes<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JByteArray<'caller> {
    unowned
        .with_env(|env| -> Result<JByteArray<'caller>> {
            let uid = uid_arg(uid)?;
            blocking(async {
                mailcore::export::prepare(crate::db::shared_db()?, folder_id, uid)
                    .await
                    .map_err(anyhow::Error::msg)
            })?;
            let eml = mailcore::export::assemble_eml(crate::db::shared_db()?, folder_id, uid)?;
            Ok(env.byte_array_from_slice(&eml)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
