//! Typed CRUD over the SQLite schema. Every submodule owns one table;
//! cross-table flows compose them via [`crate::db::Db`].

pub mod account_form;
pub mod account_settings;
pub mod accounts;
pub mod contacts;
pub mod folders;
pub mod messages;
pub mod pending_moves;
pub mod queue;
pub mod settings;

use crate::error::Result;

/// Current UTC time as RFC3339.
pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Decode a JSON-encoded string vec column.
pub(crate) fn json_vec(raw: &str) -> Result<Vec<String>> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(raw)?)
}

/// [`json_vec`] for a row mapper: a column that does not parse reads as
/// empty, with a warning naming the row and column. Silently empty hid a
/// corrupt row (A8). The value is not logged: these columns hold addresses.
pub(crate) fn json_vec_logged(raw: &str, table: &str, column: &str, id: i64) -> Vec<String> {
    json_vec(raw).unwrap_or_else(|e| {
        log::warn!("{table} {id}: {column} is not a JSON list, read as empty: {e}");
        Vec::new()
    })
}

#[must_use]
pub(crate) fn opt_bool(v: i64) -> bool {
    v != 0
}
