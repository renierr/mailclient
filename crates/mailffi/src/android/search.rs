//! Search, similar messages and date quick-filters (step 0e).

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;

/// Step 0e search/contacts/settings/misc (`android/PLAN.md`): the last JNI
/// slice — everything the list, composer, contacts, settings, outbox and
/// reader screens need beyond 0a–0d. Same translation-only rule throughout.
///
/// `MailNative.searchJson(accountId, query, folder)`: FTS over
/// subject/sender/recipients/body; `folder` is an IMAP-path scope or `""`
/// for the account. Blank/operator-only queries answer `[]`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_searchJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    query: JString<'caller>,
    folder: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::search_json(
                account_id,
                string(env, &query)?,
                string(env, &folder)?,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.searchServer(accountId, query, folder)`: queued IMAP SEARCH
/// backfill into the cache; re-run `searchJson` when the `Search` job
/// finishes.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_searchServer<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    query: JString<'caller>,
    folder: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::search::search_server(
                account_id,
                string(env, &query)?,
                string(env, &folder)?,
            )?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.searchPlan(query)`: what the field does with its text —
/// `{"mode":"off"|"filter"|"indexed","query","hit_limit","debounce_ms"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_searchPlan<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    query: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let p = crate::api::search::search_plan(string(env, &query)?);
            let mode = match p.mode {
                crate::api::search::SearchMode::Off => "off",
                crate::api::search::SearchMode::Filter => "filter",
                crate::api::search::SearchMode::Indexed => "indexed",
            };
            let json = serde_json::json!({
                "mode": mode,
                "query": p.query,
                "hit_limit": p.hit_limit,
                "debounce_ms": p.debounce_ms,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.dateRangeCheck(after, before)`: a typed custom date range,
/// normalised or refused — `{"after","before","error"}` (`error` `""` = ok).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_dateRangeCheck<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    after: JString<'caller>,
    before: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = mailcore::search::list_filter::date_range_check(
                &string(env, &after)?,
                &string(env, &before)?,
            );
            Ok(env.new_string(serde_json::to_string(&r)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.datePresetRange(preset)`: `today`/`week`/`month`/`older_month`
/// as `{"after","before"}` (`""` = unset).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_datePresetRange<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    preset: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::search::date_preset_range(string(env, &preset)?);
            let json = serde_json::json!({
                "after": r.after,
                "before": r.before,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.dateFilterLabel(after, before)`: the words for an active date
/// quick-filter.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_dateFilterLabel<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    after: JString<'caller>,
    before: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::date_filter_label(
                string(env, &after)?,
                string(env, &before)?,
            ))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.searchSyntaxHelp()`: the query-language tooltip text.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_searchSyntaxHelp<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::search_syntax_help())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.similarJson(accountId, folderId, uid)`: similar messages,
/// account-wide, best-first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_similarJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::similar_json(
                account_id,
                folder_id,
                i64::from(uid_arg(uid)?),
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.similarSubject(accountId, folderId, uid)`: the "Similar to"
/// chip text.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_similarSubject<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::similar_subject(
                account_id,
                folder_id,
                i64::from(uid_arg(uid)?),
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
