//! App settings: typed key/value preferences in the `settings` table.
//!
//! Secrets never belong here (OS keyring only). Unknown keys return `None`;
//! known keys fall back to [`defaults`] when unset.

use rusqlite::{params, OptionalExtension};

use crate::db::Db;
use crate::error::Result;

/// Save a sent-mail copy into the Sent folder (default: on).
pub const SENT_COPY_ENABLED: &str = "sent_copy_enabled";
/// Load remote images in HTML mail (default: off — privacy).
pub const LOAD_REMOTE_IMAGES: &str = "load_remote_images";
/// Outgoing format: `auto` (default, smart) | `plain` | `multipart` | `html`.
/// Unknown/empty values fall back to `auto`.
pub const COMPOSE_SEND_FORMAT: &str = "compose_send_format";
/// Always include a plain-text twin next to HTML (`1`/`0`, default: on).
/// Applies when the effective shape would be HTML-only (Auto with formatting,
/// or explicit HTML): on sends `multipart/alternative` instead.
pub const COMPOSE_INCLUDE_PLAIN: &str = "compose_include_plain";
/// Automatically mark a message read when viewed (default: on).
pub const AUTO_MARK_READ: &str = "auto_mark_read";
/// Delay in seconds before an opened message counts as read (default: `0` =
/// immediately, Thunderbird-style; capped at 300).
pub const MARK_READ_DELAY_SECS: &str = "mark_read_delay_secs";
/// Collect recipients of successfully sent mail for address suggestions
/// (default: on).
pub const COLLECT_SENT_CONTACTS: &str = "collect_sent_contacts";
/// Message list sort field: `date` (default) | `from` | `subject`.
/// Unknown/empty values fall back to `date`.
pub const MESSAGE_SORT_FIELD: &str = "message_sort_field";
/// Message list sort direction (`1` = descending/newest-first, default;
/// `0` = ascending).
pub const MESSAGE_SORT_DESC: &str = "message_sort_desc";
/// Ask before moving mail to Trash, single and bulk (default: on).
/// Purge (permanent delete) always asks, independently of this.
pub const CONFIRM_DELETE: &str = "confirm_delete";
/// Message list density: `comfortable` (default) | `compact`.
/// Unknown/empty values fall back to `comfortable`.
pub const LIST_DENSITY: &str = "list_density";
/// Plain-text reader size: `small` | `normal` (default) | `large`.
/// Unknown/empty values fall back to `normal`.
pub const READER_FONT_SIZE: &str = "reader_font_size";
/// Automatic mail check, in minutes (`0` = manually only, default).
/// Clamped to 0..1440; the UI offers fixed steps.
pub const SYNC_INTERVAL_MINUTES: &str = "sync_interval_minutes";
/// Append the signature to new mail, replies and forwards (default: off).
pub const SIGNATURE_ENABLED: &str = "signature_enabled";
/// Plain-text signature body (default: empty).
pub const SIGNATURE_TEXT: &str = "signature_text";
/// Place the reply below the quote instead of above it (default: off).
pub const REPLY_BELOW_QUOTE: &str = "reply_below_quote";
/// Request a read receipt (`Disposition-Notification-To`, default: off).
/// Recipients may ignore it; it only asks.
pub const REQUEST_MDN: &str = "request_mdn";
/// Interface scale factor (`1` = 100%, default). Snapped to the supported
/// steps `1` | `1.1` | `1.25` | `1.5`; unknown values fall back to `1`.
pub const UI_SCALE: &str = "ui_scale";
/// Last account selected in the UI. Absent/invalid values deliberately leave
/// startup selection to the normal first-account fallback.
pub const LAST_ACTIVE_ACCOUNT_ID: &str = "last_active_account_id";
/// Prefix for per-account full-discovery timestamps (unix seconds, internal
/// bookkeeping for the discovery throttle — not a user preference, no
/// default): `last_full_discovery_{account_id}`.
pub const LAST_FULL_DISCOVERY_PREFIX: &str = "last_full_discovery_";

/// Built-in default for a known key, if any.
#[must_use]
pub fn defaults(key: &str) -> Option<&'static str> {
    match key {
        SENT_COPY_ENABLED => Some("1"),
        LOAD_REMOTE_IMAGES => Some("0"),
        COMPOSE_SEND_FORMAT => Some("auto"),
        COMPOSE_INCLUDE_PLAIN => Some("1"),
        AUTO_MARK_READ => Some("1"),
        MARK_READ_DELAY_SECS => Some("0"),
        COLLECT_SENT_CONTACTS => Some("1"),
        MESSAGE_SORT_FIELD => Some("date"),
        MESSAGE_SORT_DESC => Some("1"),
        CONFIRM_DELETE => Some("1"),
        LIST_DENSITY => Some("comfortable"),
        READER_FONT_SIZE => Some("normal"),
        SYNC_INTERVAL_MINUTES => Some("0"),
        SIGNATURE_ENABLED => Some("0"),
        SIGNATURE_TEXT => Some(""),
        REPLY_BELOW_QUOTE => Some("0"),
        REQUEST_MDN => Some("0"),
        UI_SCALE => Some("1"),
        _ => None,
    }
}

/// Raw value, if set.
pub fn get(db: &Db, key: &str) -> Result<Option<String>> {
    Ok(db
        .conn()
        .query_row("select value from settings where key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

/// Boolean value: stored `1`/`true` (case-insensitive), else built-in default,
/// else `false` for unknown keys.
pub fn get_bool(db: &Db, key: &str) -> Result<bool> {
    match get(db, key)? {
        Some(v) => Ok(v == "1" || v.eq_ignore_ascii_case("true")),
        None => Ok(defaults(key).is_some_and(|d| d == "1")),
    }
}

/// Store a raw value (insert or overwrite).
pub fn set(db: &Db, key: &str, value: &str) -> Result<()> {
    db.conn().execute(
        "insert into settings (key, value) values (?1, ?2)
         on conflict (key) do update set value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Store a boolean value (`1`/`0`).
pub fn set_bool(db: &Db, key: &str, value: bool) -> Result<()> {
    set(db, key, if value { "1" } else { "0" })
}

/// Last selected account ID, if it is a valid positive SQLite row ID.
pub fn get_last_active_account_id(db: &Db) -> Option<i64> {
    get(db, LAST_ACTIVE_ACCOUNT_ID)
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|id| *id > 0)
}

/// Persist the account the user is actively viewing.
pub fn set_last_active_account_id(db: &Db, account_id: i64) -> Result<()> {
    set(db, LAST_ACTIVE_ACCOUNT_ID, &account_id.max(0).to_string())
}

/// Last full folder discovery (unix seconds) for one account, if any.
pub fn get_last_full_discovery(db: &Db, account_id: i64) -> Option<i64> {
    get(db, &format!("{LAST_FULL_DISCOVERY_PREFIX}{account_id}"))
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|secs| *secs > 0)
}

/// Stamp a full folder discovery (unix seconds) for one account.
pub fn set_last_full_discovery(db: &Db, account_id: i64, unix_secs: i64) -> Result<()> {
    set(
        db,
        &format!("{LAST_FULL_DISCOVERY_PREFIX}{account_id}"),
        &unix_secs.max(0).to_string(),
    )
}

/// Outgoing send format, resilient: unknown values become `auto`.
#[must_use]
pub fn normalize_send_format(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "plain" => "plain",
        "multipart" => "multipart",
        "html" => "html",
        _ => "auto",
    }
}

/// Validated outgoing format for the settings store.
pub fn get_send_format(db: &Db) -> String {
    match get(db, COMPOSE_SEND_FORMAT) {
        Ok(Some(v)) => normalize_send_format(&v).to_string(),
        _ => defaults(COMPOSE_SEND_FORMAT).unwrap_or("auto").to_string(),
    }
}

/// Clamp a mark-as-read delay into the sane range (seconds).
#[must_use]
pub fn normalize_delay_secs(raw: i64) -> i64 {
    raw.clamp(0, 300)
}

/// Delay in seconds before an opened message counts as read.
/// Unset/unparseable values fall back to the built-in default.
pub fn get_delay_secs(db: &Db, key: &str) -> i64 {
    match get(db, key) {
        Ok(Some(v)) => v
            .trim()
            .parse::<i64>()
            .map(normalize_delay_secs)
            .unwrap_or_else(|_| {
                defaults(key)
                    .and_then(|d| d.parse::<i64>().ok())
                    .unwrap_or(0)
            }),
        _ => defaults(key)
            .and_then(|d| d.parse::<i64>().ok())
            .unwrap_or(0),
    }
}

/// Store a mark-as-read delay (normalized first).
pub fn set_delay_secs(db: &Db, key: &str, value: i64) -> Result<()> {
    set(db, key, &normalize_delay_secs(value).to_string())
}

/// Validated message-list sort field: `date` | `from` | `subject`.
/// Unknown/empty values fall back to `date` (Roundcube offers the same three
/// primary orderings).
#[must_use]
pub fn normalize_sort_field(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "from" | "from_addr" | "sender" => "from",
        "subject" => "subject",
        _ => "date",
    }
}

/// Current message-list sort field, resilient to unknown stored values.
pub fn get_sort_field(db: &Db) -> String {
    match get(db, MESSAGE_SORT_FIELD) {
        Ok(Some(v)) => normalize_sort_field(&v).to_string(),
        _ => defaults(MESSAGE_SORT_FIELD).unwrap_or("date").to_string(),
    }
}

/// Current message-list sort direction: `true` = descending (newest/Z-A first).
pub fn get_sort_descending(db: &Db) -> bool {
    match get(db, MESSAGE_SORT_DESC) {
        Ok(Some(v)) => !(v == "0" || v.eq_ignore_ascii_case("false")),
        _ => true,
    }
}

/// Persist the message-list sort (`field` is normalized first).
pub fn set_sort(db: &Db, field: &str, descending: bool) -> Result<()> {
    set(db, MESSAGE_SORT_FIELD, normalize_sort_field(field))?;
    set_bool(db, MESSAGE_SORT_DESC, descending)
}

/// Validated list density: `comfortable` | `compact`.
#[must_use]
pub fn normalize_density(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "compact" => "compact",
        _ => "comfortable",
    }
}

/// Current list density, resilient to unknown stored values.
pub fn get_density(db: &Db) -> String {
    match get(db, LIST_DENSITY) {
        Ok(Some(v)) => normalize_density(&v).to_string(),
        _ => defaults(LIST_DENSITY).unwrap_or("comfortable").to_string(),
    }
}

/// Validated reader size: `small` | `normal` | `large`.
#[must_use]
pub fn normalize_reader_font(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "small" => "small",
        "large" => "large",
        _ => "normal",
    }
}

/// Current reader size, resilient to unknown stored values.
pub fn get_reader_font(db: &Db) -> String {
    match get(db, READER_FONT_SIZE) {
        Ok(Some(v)) => normalize_reader_font(&v).to_string(),
        _ => defaults(READER_FONT_SIZE).unwrap_or("normal").to_string(),
    }
}

/// Clamp an auto-check interval into the sane range (minutes, 0 = manual).
#[must_use]
pub fn normalize_sync_interval(raw: i64) -> i64 {
    raw.clamp(0, 1440)
}

/// Automatic mail-check interval in minutes (`0` = manually only).
/// Unset/unparseable values fall back to the built-in default.
pub fn get_sync_interval(db: &Db) -> i64 {
    match get(db, SYNC_INTERVAL_MINUTES) {
        Ok(Some(v)) => v
            .trim()
            .parse::<i64>()
            .map(normalize_sync_interval)
            .unwrap_or_else(|_| {
                defaults(SYNC_INTERVAL_MINUTES)
                    .and_then(|d| d.parse::<i64>().ok())
                    .unwrap_or(0)
            }),
        _ => defaults(SYNC_INTERVAL_MINUTES)
            .and_then(|d| d.parse::<i64>().ok())
            .unwrap_or(0),
    }
}

/// Store an auto-check interval (normalized first).
pub fn set_sync_interval(db: &Db, value: i64) -> Result<()> {
    set(
        db,
        SYNC_INTERVAL_MINUTES,
        &normalize_sync_interval(value).to_string(),
    )
}

/// Plain-text signature body (`""` when unset).
pub fn get_signature_text(db: &Db) -> String {
    match get(db, SIGNATURE_TEXT) {
        Ok(Some(v)) => v,
        _ => String::new(),
    }
}

/// Snap an interface scale into the supported steps
/// (`1` | `1.1` | `1.25` | `1.5`). Everything above snaps back down —
/// fixed control boxes are audited up to 150%.
#[must_use]
pub fn normalize_ui_scale(raw: f32) -> f32 {
    if !raw.is_finite() {
        return 1.0;
    }
    if raw < 1.05 {
        1.0
    } else if raw < 1.175 {
        1.1
    } else if raw < 1.375 {
        1.25
    } else {
        1.5
    }
}

/// Current interface scale, resilient to unknown stored values.
pub fn get_ui_scale(db: &Db) -> f32 {
    match get(db, UI_SCALE) {
        Ok(Some(v)) => v
            .trim()
            .parse::<f32>()
            .map(normalize_ui_scale)
            .unwrap_or(1.0),
        _ => 1.0,
    }
}

/// Store an interface scale (snapped first).
pub fn set_ui_scale(db: &Db, value: f32) -> Result<()> {
    let snapped = normalize_ui_scale(value);
    set(
        db,
        UI_SCALE,
        if snapped == 1.1 {
            "1.1"
        } else if snapped == 1.25 {
            "1.25"
        } else if snapped == 1.5 {
            "1.5"
        } else {
            "1"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_format_resilient() {
        assert_eq!(normalize_send_format("plain"), "plain");
        assert_eq!(normalize_send_format("multipart"), "multipart");
        assert_eq!(normalize_send_format(" HTML "), "html");
        assert_eq!(normalize_send_format("auto"), "auto");
        assert_eq!(normalize_send_format("weird"), "auto");
        assert_eq!(normalize_send_format(""), "auto");
        let db = Db::open_in_memory().unwrap();
        assert_eq!(get_send_format(&db), "auto");
        assert!(get_bool(&db, COMPOSE_INCLUDE_PLAIN).unwrap());
        set(&db, COMPOSE_SEND_FORMAT, "plain").unwrap();
        assert_eq!(get_send_format(&db), "plain");
        set(&db, COMPOSE_SEND_FORMAT, "nonsense").unwrap();
        assert_eq!(get_send_format(&db), "auto");
    }

    #[test]
    fn mark_read_defaults_and_delay_bounds() {
        assert_eq!(normalize_delay_secs(-5), 0);
        assert_eq!(normalize_delay_secs(5), 5);
        assert_eq!(normalize_delay_secs(9999), 300);
        let db = Db::open_in_memory().unwrap();
        assert!(get_bool(&db, AUTO_MARK_READ).unwrap());
        assert!(get_bool(&db, COLLECT_SENT_CONTACTS).unwrap());
        assert_eq!(get_delay_secs(&db, MARK_READ_DELAY_SECS), 0);
        set_delay_secs(&db, MARK_READ_DELAY_SECS, 10).unwrap();
        assert_eq!(get_delay_secs(&db, MARK_READ_DELAY_SECS), 10);
        set(&db, MARK_READ_DELAY_SECS, "nonsense").unwrap();
        assert_eq!(get_delay_secs(&db, MARK_READ_DELAY_SECS), 0);
        set_bool(&db, AUTO_MARK_READ, false).unwrap();
        assert!(!get_bool(&db, AUTO_MARK_READ).unwrap());
    }

    #[test]
    fn defaults_apply_and_overrides_win() {
        let db = Db::open_in_memory().unwrap();
        // Built-in defaults, nothing stored yet.
        assert!(get_bool(&db, SENT_COPY_ENABLED).unwrap());
        assert!(!get_bool(&db, LOAD_REMOTE_IMAGES).unwrap());
        assert!(!get_bool(&db, "nope.unknown").unwrap());

        set_bool(&db, SENT_COPY_ENABLED, false).unwrap();
        assert!(!get_bool(&db, SENT_COPY_ENABLED).unwrap());
        set(&db, SENT_COPY_ENABLED, "true").unwrap();
        assert!(get_bool(&db, SENT_COPY_ENABLED).unwrap());
        assert_eq!(
            get(&db, SENT_COPY_ENABLED).unwrap().as_deref(),
            Some("true")
        );
        assert_eq!(get(&db, "missing").unwrap(), None);
    }

    #[test]
    fn last_active_account_id_is_resilient_and_persisted() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(get_last_active_account_id(&db), None);
        set_last_active_account_id(&db, 42).unwrap();
        assert_eq!(get_last_active_account_id(&db), Some(42));
        set(&db, LAST_ACTIVE_ACCOUNT_ID, "nonsense").unwrap();
        assert_eq!(get_last_active_account_id(&db), None);
        set(&db, LAST_ACTIVE_ACCOUNT_ID, "-1").unwrap();
        assert_eq!(get_last_active_account_id(&db), None);
    }

    #[test]
    fn full_discovery_stamp_round_trips_per_account() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(get_last_full_discovery(&db, 1), None);
        set_last_full_discovery(&db, 1, 1700000000).unwrap();
        assert_eq!(get_last_full_discovery(&db, 1), Some(1700000000));
        assert_eq!(get_last_full_discovery(&db, 2), None);
        set(&db, "last_full_discovery_1", "nonsense").unwrap();
        assert_eq!(get_last_full_discovery(&db, 1), None);
    }

    #[test]
    fn message_sort_resilient_and_persisted() {
        assert_eq!(normalize_sort_field("date"), "date");
        assert_eq!(normalize_sort_field(" From "), "from");
        assert_eq!(normalize_sort_field("sender"), "from");
        assert_eq!(normalize_sort_field("SUBJECT"), "subject");
        assert_eq!(normalize_sort_field("size"), "date");
        assert_eq!(normalize_sort_field(""), "date");
        let db = Db::open_in_memory().unwrap();
        assert_eq!(get_sort_field(&db), "date");
        assert!(get_sort_descending(&db));
        set_sort(&db, "subject", false).unwrap();
        assert_eq!(get_sort_field(&db), "subject");
        assert!(!get_sort_descending(&db));
        set(&db, MESSAGE_SORT_FIELD, "nonsense").unwrap();
        assert_eq!(get_sort_field(&db), "date");
    }

    #[test]
    fn mailbox_and_compose_prefs_resilient() {
        assert_eq!(normalize_density("compact"), "compact");
        assert_eq!(normalize_density(" COMFORTABLE "), "comfortable");
        assert_eq!(normalize_density("cozy"), "comfortable");
        assert_eq!(normalize_reader_font("small"), "small");
        assert_eq!(normalize_reader_font("LARGE"), "large");
        assert_eq!(normalize_reader_font("huge"), "normal");
        assert_eq!(normalize_sync_interval(-5), 0);
        assert_eq!(normalize_sync_interval(15), 15);
        assert_eq!(normalize_sync_interval(99999), 1440);
        let db = Db::open_in_memory().unwrap();
        assert!(get_bool(&db, CONFIRM_DELETE).unwrap());
        assert_eq!(get_density(&db), "comfortable");
        assert_eq!(get_reader_font(&db), "normal");
        assert_eq!(get_sync_interval(&db), 0);
        assert_eq!(get_signature_text(&db), "");
        assert!(!get_bool(&db, SIGNATURE_ENABLED).unwrap());
        assert!(!get_bool(&db, REPLY_BELOW_QUOTE).unwrap());
        assert!(!get_bool(&db, REQUEST_MDN).unwrap());
        set(&db, LIST_DENSITY, "weird").unwrap();
        assert_eq!(get_density(&db), "comfortable");
        set_sync_interval(&db, 15).unwrap();
        assert_eq!(get_sync_interval(&db), 15);
        set(&db, SYNC_INTERVAL_MINUTES, "nonsense").unwrap();
        assert_eq!(get_sync_interval(&db), 0);
        set(&db, SIGNATURE_TEXT, "Kind regards").unwrap();
        assert_eq!(get_signature_text(&db), "Kind regards");
    }

    #[test]
    fn ui_scale_snaps_to_supported_steps() {
        assert_eq!(normalize_ui_scale(1.0), 1.0);
        assert_eq!(normalize_ui_scale(1.1), 1.1);
        assert_eq!(normalize_ui_scale(1.25), 1.25);
        assert_eq!(normalize_ui_scale(1.5), 1.5);
        assert_eq!(normalize_ui_scale(0.5), 1.0);
        assert_eq!(normalize_ui_scale(2.0), 1.5);
        assert_eq!(normalize_ui_scale(f32::NAN), 1.0);
        let db = Db::open_in_memory().unwrap();
        assert_eq!(get_ui_scale(&db), 1.0);
        set_ui_scale(&db, 1.5).unwrap();
        assert_eq!(get_ui_scale(&db), 1.5);
        set_ui_scale(&db, 2.0).unwrap();
        assert_eq!(get_ui_scale(&db), 1.5);
        set(&db, UI_SCALE, "nonsense").unwrap();
        assert_eq!(get_ui_scale(&db), 1.0);
    }
}
