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

/// Run `f` atomically: in an `IMMEDIATE` transaction of its own, or inside
/// the caller's when one is already open (a nested `BEGIN` would fail). An
/// error rolls the whole of `f` back, so a multi-statement write never
/// stops half done (A9, A10).
pub(crate) fn atomic<T>(db: &crate::db::Db, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let conn = db.conn();
    if !conn.is_autocommit() {
        return f();
    }
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let value = f()?;
    tx.commit()?;
    Ok(value)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::error::StoreError;

    fn keys(db: &Db) -> i64 {
        db.conn()
            .query_row(
                "select count(*) from settings where key like 't.%'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn atomic_rolls_back_everything_on_error_and_nests() {
        let db = Db::open_in_memory().unwrap();
        let put = |k: &str| {
            db.conn()
                .execute("insert into settings (key, value) values (?1, 'v')", [k])
                .map(|_| ())
                .map_err(StoreError::from)
        };
        let failed: Result<()> = atomic(&db, || {
            put("t.a")?;
            Err(StoreError::InvalidInput("boom".into()))
        });
        assert!(failed.is_err());
        assert_eq!(keys(&db), 0, "the first write was rolled back");
        assert!(db.conn().is_autocommit());

        // Inside a caller's transaction it joins instead of failing to BEGIN.
        let tx = db.conn().unchecked_transaction().unwrap();
        atomic(&db, || put("t.b")).unwrap();
        tx.commit().unwrap();
        assert_eq!(keys(&db), 1);
    }
}
