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
/// Outgoing format: `plain` | `multipart` (default, resilient) | `html`.
/// Unknown/empty values fall back to `multipart`.
pub const COMPOSE_SEND_FORMAT: &str = "compose_send_format";
/// Automatically mark a message read when viewed (default: on).
pub const AUTO_MARK_READ: &str = "auto_mark_read";
/// Delay in seconds before an opened message counts as read (default: `0` =
/// immediately, Thunderbird-style; capped at 300).
pub const MARK_READ_DELAY_SECS: &str = "mark_read_delay_secs";

/// Built-in default for a known key, if any.
#[must_use]
pub fn defaults(key: &str) -> Option<&'static str> {
    match key {
        SENT_COPY_ENABLED => Some("1"),
        LOAD_REMOTE_IMAGES => Some("0"),
        COMPOSE_SEND_FORMAT => Some("multipart"),
        AUTO_MARK_READ => Some("1"),
        MARK_READ_DELAY_SECS => Some("0"),
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

/// Outgoing send format, resilient: unknown values become `multipart`.
#[must_use]
pub fn normalize_send_format(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "plain" => "plain",
        "html" => "html",
        _ => "multipart",
    }
}

/// Validated outgoing format for the settings store.
pub fn get_send_format(db: &Db) -> String {
    match get(db, COMPOSE_SEND_FORMAT) {
        Ok(Some(v)) => normalize_send_format(&v).to_string(),
        _ => defaults(COMPOSE_SEND_FORMAT)
            .unwrap_or("multipart")
            .to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_format_resilient() {
        assert_eq!(normalize_send_format("plain"), "plain");
        assert_eq!(normalize_send_format(" HTML "), "html");
        assert_eq!(normalize_send_format("weird"), "multipart");
        assert_eq!(normalize_send_format(""), "multipart");
        let db = Db::open_in_memory().unwrap();
        assert_eq!(get_send_format(&db), "multipart");
        set(&db, COMPOSE_SEND_FORMAT, "plain").unwrap();
        assert_eq!(get_send_format(&db), "plain");
        set(&db, COMPOSE_SEND_FORMAT, "nonsense").unwrap();
        assert_eq!(get_send_format(&db), "multipart");
    }

    #[test]
    fn mark_read_defaults_and_delay_bounds() {
        assert_eq!(normalize_delay_secs(-5), 0);
        assert_eq!(normalize_delay_secs(5), 5);
        assert_eq!(normalize_delay_secs(9999), 300);
        let db = Db::open_in_memory().unwrap();
        assert!(get_bool(&db, AUTO_MARK_READ).unwrap());
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
}
