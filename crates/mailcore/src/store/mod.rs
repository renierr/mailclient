//! Typed CRUD over the SQLite schema. Every submodule owns one table;
//! cross-table flows compose them via [`crate::db::Db`].

pub mod accounts;
pub mod contacts;
pub mod folders;
pub mod messages;
pub mod queue;

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

#[must_use]
pub(crate) fn opt_bool(v: i64) -> bool {
    v != 0
}
