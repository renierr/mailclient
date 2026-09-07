//! App settings: typed key/value preferences in the `settings` table.
//!
//! Secrets never belong here (OS keyring only). Unknown keys return `None`;
//! known keys fall back to [`defaults`] when unset.

use rusqlite::{OptionalExtension, params};

use crate::db::Db;
use crate::error::Result;

/// Save a sent-mail copy into the Sent folder (default: on).
pub const SENT_COPY_ENABLED: &str = "sent_copy_enabled";
/// Load remote images in HTML mail (default: off — privacy).
pub const LOAD_REMOTE_IMAGES: &str = "load_remote_images";

/// Built-in default for a known key, if any.
#[must_use]
pub fn defaults(key: &str) -> Option<&'static str> {
    match key {
        SENT_COPY_ENABLED => Some("1"),
        LOAD_REMOTE_IMAGES => Some("0"),
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(get(&db, SENT_COPY_ENABLED).unwrap().as_deref(), Some("true"));
        assert_eq!(get(&db, "missing").unwrap(), None);
    }
}
