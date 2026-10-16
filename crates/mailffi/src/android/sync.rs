//! Queued sync jobs and app-foreground sync decisions.

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::JClass;
use jni::EnvUnowned;

/// `MailNative.resumeSyncDue(accountId)`: whether returning to the app
/// should sync the account (`mailcore::sync::resume::resume_sync_due`) —
/// not when it finished a sync within the grace period.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_resumeSyncDue<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) -> bool {
    unowned
        .with_env(|_env| -> Result<bool> {
            Ok(mailcore::sync::resume::resume_sync_due(
                crate::db::shared_db()?,
                account_id,
            ))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.syncAccount(accountId)`: queue a full account sync; returns
/// at once, the result arrives as a `Sync` finished event. Throws when a
/// sync for the account is already queued (`spawn` dedupe).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_syncAccount<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::sync_account(account_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.syncFolder(accountId, folderId)`: fill one opened folder's
/// newest window, after the cached rows have painted.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_syncFolder<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::sync_folder(account_id, folder_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.loadOlderMessages(accountId, folderId)`: the next older batch,
/// for the list's "Show older" row.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_loadOlderMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::load_older_messages(account_id, folder_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.refreshFolders(accountId)`: LIST only — new/renamed/deleted
/// folders appear. A `Folders` finished event says when.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_refreshFolders<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::refresh_folders(account_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.refreshServerCapabilities(accountId)`: the About view's
/// capability list, as the finishing event's JSON status payload.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_refreshServerCapabilities<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::refresh_server_capabilities(account_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.backgroundMarkSeen()`: the inbox cache counts as seen while
/// the app is open — no network, answers inline.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_backgroundMarkSeen<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::background_mark_seen()?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
