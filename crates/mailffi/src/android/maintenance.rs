//! Storage stats, cleanup, trims and the database export.

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;

/// `MailNative.storageStatsJson(dbPath, tempDir)`: database, message and
/// temp sizes for the maintenance section.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_storageStatsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    db_path: JString<'caller>,
    temp_dir: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::maintenance::storage_stats_json(
                string(env, &db_path)?,
                string(env, &temp_dir)?,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.cleanupTempFilesJson(tempDir)`: delete temp files now —
/// `{files_removed, bytes_freed, ...}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_cleanupTempFilesJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    temp_dir: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(
                env.new_string(crate::api::maintenance::cleanup_temp_files_json(string(
                    env, &temp_dir,
                )?)?)?,
            )
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.trimLocalCache()`: trim the local cache to the newest rows —
/// how many went, back as a string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_trimLocalCache<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::maintenance::trim_local_cache()?.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.trimStatus(removed)`: the words for a trim result.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_trimStatus<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    removed: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = u64::try_from(removed)
                .map_err(|_| BridgeError(format!("bad trim count: {removed}")))?;
            Ok(env.new_string(crate::api::maintenance::trim_status(n))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.evictCachedAttachmentsJson()`: drop downloaded attachment
/// bytes (they re-download on open) — `{files, bytes_freed, ...}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_evictCachedAttachmentsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::maintenance::evict_cached_attachments_json()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.exportDatabaseTo(path)`: a `VACUUM INTO` snapshot of the
/// database at `path` — where it went.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_exportDatabaseTo<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(
                env.new_string(crate::api::maintenance::export_database_to(string(
                    env, &path,
                )?)?)?,
            )
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
