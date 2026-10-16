//! Preferences, per-account overrides and About info.

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;

/// `MailNative.appInfoJson()`: `{version, license, db_path}` for About.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_appInfoJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let info = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "license": env!("CARGO_PKG_LICENSE"),
                "db_path": crate::db::db_path().to_string_lossy(),
            });
            Ok(env.new_string(info.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.settingsJson()`: every preference, normalized.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_settingsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::settings::settings_json()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.settingChoicesJson()`: defaults and offered values per key —
/// the settings screen labels these, in the shared words.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_settingChoicesJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::settings::setting_choices_json())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.quietTime(text)`: a typed time in picker parts —
/// `{"hour","minute"}`, `""` when it does not read as one.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_quietTime<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    text: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let out = match crate::api::settings::quiet_time(string(env, &text)?) {
                Some(t) => serde_json::json!({"hour": t.hour, "minute": t.minute}).to_string(),
                None => String::new(),
            };
            Ok(env.new_string(out)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.quietTimeAt(hour, minute)`: the stored `"HH:MM"` form of a
/// picked time, `""` when out of range.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_quietTimeAt<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    hour: i32,
    minute: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let out = crate::api::settings::quiet_time_at(
                u32::try_from(hour.max(0)).unwrap_or(u32::MAX),
                u32::try_from(minute.max(0)).unwrap_or(u32::MAX),
            )
            .unwrap_or_default();
            Ok(env.new_string(out)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

fn string_map_json(raw: &str) -> Result<Vec<(String, String)>> {
    let v: serde_json::Value = serde_json::from_str(raw)?;
    v.as_object()
        .ok_or_else(|| BridgeError("expected a JSON string object".to_string()))?
        .iter()
        .map(|(k, val)| {
            val.as_str()
                .map(|s| (k.clone(), s.to_string()))
                .ok_or_else(|| BridgeError(format!("bad value for {k}")))
        })
        .collect()
}

/// `MailNative.setSettings(values)`: several preferences from a JSON string
/// object, in one transaction — all apply or none do.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setSettings<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    values: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let pairs = string_map_json(&string(env, &values)?)?;
            crate::api::settings::set_settings(pairs.into_iter().collect())?;
            Ok(env.new_string("saved")?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setSort(field, descending)`: the list ordering, normalized as
/// a pair.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setSort<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    field: JString<'caller>,
    descending: bool,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::settings::set_sort(string(env, &field)?, descending)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.accountSettingsJson(accountId)`: one account's `overrides`
/// plus the `effective` values for every overridable key.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_accountSettingsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::settings::account_settings_json(account_id)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setAccountSettings(accountId, values)`: one account's
/// overrides from a JSON string object — an empty value inherits again.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setAccountSettings<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    values: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let pairs = string_map_json(&string(env, &values)?)?;
            crate::api::settings::set_account_settings(account_id, pairs.into_iter().collect())?;
            Ok(env.new_string("saved")?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
