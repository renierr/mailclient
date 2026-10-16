//! Folder tree reads and sidebar rows (step 0a).

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;

/// `MailNative.foldersJson(accountId)`: the move picker's folder tree.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_foldersJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::folders::folders_json(account_id)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.sidebarRowsJson(accountId, expandedJson)`: painted sidebar
/// rows (`[{id, collapsible, expanded, unread, total}]`) for the expanded
/// folder ids in `expandedJson` (`[]` = all collapsed).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_sidebarRowsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    expanded_json: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let expanded = string(env, &expanded_json)?;
            Ok(env.new_string(crate::api::folders::sidebar_rows_json(
                account_id, expanded,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setFolderSubscribed(folderId, subscribed)`: show or hide a
/// folder in the sidebar (display-only, cache kept).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setFolderSubscribed<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    subscribed: bool,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::folders::set_folder_subscribed(folder_id, subscribed)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.folderCounts(folderId)`: local vs server totals plus the "Show
/// older" state — `{"cached","server","older","can_load_older"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_folderCounts<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let c = crate::api::folders::folder_counts(folder_id)?;
            let older = match c.older {
                crate::api::folders::OlderState::Unchecked => "unchecked",
                crate::api::folders::OlderState::Partial => "partial",
                crate::api::folders::OlderState::Empty => "empty",
                crate::api::folders::OlderState::Complete => "complete",
            };
            let json = serde_json::json!({
                "cached": c.cached,
                "server": c.server,
                "older": older,
                "can_load_older": c.can_load_older,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
