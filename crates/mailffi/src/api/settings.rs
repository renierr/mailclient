//! User preferences.
//!
//! The Qt frontend mirrors each preference into a typed `SettingsBridge`
//! property, because that is how QML binds. Dart does not need that: the
//! whole set crosses once as JSON and a Dart settings model holds it.
//!
//! Values are read back through `mailcore`'s normalizers rather than raw, so
//! what the UI shows is what the app will actually do with a stale or
//! hand-edited row — `list_density = "tiny"` reads back as `comfortable`.

use mailcore::store::settings as s;

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
        s::SYNC_INTERVAL_MINUTES: s::get_sync_interval(db),
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
    Ok(s::set(shared_db()?, &key, &value)?)
}

/// Message-list ordering. Separate from [`set_setting`] because the two keys
/// are only meaningful together, and `mailcore` normalizes the pair.
pub fn set_sort(field: String, descending: bool) -> anyhow::Result<()> {
    Ok(s::set_sort(shared_db()?, &field, descending)?)
}
