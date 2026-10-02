//! User preferences.
//!
//! The Qt frontend mirrors each preference into a typed `SettingsBridge`
//! property, because that is how QML binds. Dart does not need that: the
//! whole set crosses once as JSON and a Dart settings model holds it.
//!
//! Values are read back through `mailcore`'s normalizers rather than raw, so
//! what the UI shows is what the app will actually do with a stale or
//! hand-edited row — `list_density = "tiny"` reads back as `comfortable`.

use mailcore::store::account_settings;
use mailcore::store::settings as s;
use mailcore::sync::background::schedule;

use crate::db::shared_db;

/// Every preference, normalized, as a JSON object.
pub fn settings_json() -> anyhow::Result<String> {
    let db = shared_db()?;
    let flag = |k: &str| s::get_bool(db, k).unwrap_or(false);
    Ok(serde_json::json!({
        s::SENT_COPY_ENABLED: flag(s::SENT_COPY_ENABLED),
        s::LOAD_REMOTE_IMAGES: flag(s::LOAD_REMOTE_IMAGES),
        s::COMPOSE_SEND_FORMAT: s::get_send_format(db),
        s::COMPOSE_INCLUDE_PLAIN: flag(s::COMPOSE_INCLUDE_PLAIN),
        s::AUTO_MARK_READ: flag(s::AUTO_MARK_READ),
        s::MARK_READ_DELAY_SECS: s::get_delay_secs(db, s::MARK_READ_DELAY_SECS),
        s::COLLECT_SENT_CONTACTS: flag(s::COLLECT_SENT_CONTACTS),
        s::CONFIRM_DELETE: flag(s::CONFIRM_DELETE),
        s::LIST_DENSITY: s::get_density(db),
        s::READER_FONT_SIZE: s::get_reader_font(db),
        s::LINK_CLICK_ACTION: s::get_link_click(db),
        s::SYNC_INTERVAL_MINUTES: s::get_sync_interval(db),
        s::BACKGROUND_SCHEDULER: s::get_background_scheduler(db),
        s::NOTIFICATIONS_ENABLED: flag(s::NOTIFICATIONS_ENABLED),
        s::QUIET_HOURS_ENABLED: flag(s::QUIET_HOURS_ENABLED),
        s::QUIET_HOURS_START: s::get_quiet_time(db, s::QUIET_HOURS_START),
        s::QUIET_HOURS_END: s::get_quiet_time(db, s::QUIET_HOURS_END),
        s::SIGNATURE_ENABLED: flag(s::SIGNATURE_ENABLED),
        s::SIGNATURE_TEXT: s::get_signature_text(db),
        s::REPLY_BELOW_QUOTE: flag(s::REPLY_BELOW_QUOTE),
        s::REQUEST_MDN: flag(s::REQUEST_MDN),
        s::UI_SCALE: s::get_ui_scale(db),
        s::MESSAGE_SORT_FIELD: s::get_sort_field(db),
        s::MESSAGE_SORT_DESC: s::get_sort_descending(db),
    })
    .to_string())
}

/// Write one preference. `value` is the raw string form (`"1"`/`"0"` for
/// flags); `mailcore` clamps and normalizes on the way back out.
///
/// Rejects unknown keys rather than writing them: a typo that silently
/// persists is a setting that silently never applies.
pub fn set_setting(key: String, value: String) -> anyhow::Result<()> {
    if s::defaults(&key).is_none() {
        anyhow::bail!("unknown setting: {key}");
    }
    Ok(s::set_many(shared_db()?, &[(key, value)])?)
}

/// Write several preferences in one transaction: all apply or none do.
/// `values` maps key to raw string value; unknown keys fail the whole batch.
pub fn set_settings(values: std::collections::HashMap<String, String>) -> anyhow::Result<()> {
    let pairs: Vec<(String, String)> = values.into_iter().collect();
    Ok(s::set_many(shared_db()?, &pairs)?)
}

/// Message-list ordering. Separate from [`set_setting`] because the two keys
/// are only meaningful together, and `mailcore` normalizes the pair.
pub fn set_sort(field: String, descending: bool) -> anyhow::Result<()> {
    Ok(s::set_sort(shared_db()?, &field, descending)?)
}

/// One account's settings as JSON: `overrides` holds only what the account
/// sets itself, `effective` what applies to it for every overridable key.
/// Both use the app-wide key names and string values (`"1"`/`"0"`, minutes).
pub fn account_settings_json(account_id: i64) -> anyhow::Result<String> {
    Ok(serde_json::to_string(&account_settings::view(
        shared_db()?,
        account_id,
    )?)?)
}

/// Write several of one account's overrides at once: all apply or none do.
/// An empty value inherits the app-wide setting again.
pub fn set_account_settings(
    account_id: i64,
    values: std::collections::HashMap<String, String>,
) -> anyhow::Result<()> {
    let pairs: Vec<(String, String)> = values.into_iter().collect();
    Ok(account_settings::set_overrides(
        shared_db()?,
        account_id,
        &pairs,
    )?)
}

/// What the Android host should run in the background, as JSON
/// (`push`, `poll_minutes`, `poll_scheduler`).
pub fn background_plan_json() -> anyhow::Result<String> {
    Ok(serde_json::to_string(&schedule::plan(shared_db()?))?)
}
