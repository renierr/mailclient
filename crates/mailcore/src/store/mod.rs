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

/// Integer column `idx` as `T`. A value outside `T` (a hand-edited or
/// corrupt row) reads as 0 with a warning naming the column, instead of
/// wrapping: a port stored as 70000 read back as 4464, a negative size as
/// 1.8e19 (A14).
pub(crate) fn int_col<T: TryFrom<i64> + Default>(
    row: &rusqlite::Row<'_>,
    idx: usize,
) -> rusqlite::Result<T> {
    Ok(opt_int_col(row, idx)?.unwrap_or_default())
}

/// [`int_col`] for a nullable column.
pub(crate) fn opt_int_col<T: TryFrom<i64> + Default>(
    row: &rusqlite::Row<'_>,
    idx: usize,
) -> rusqlite::Result<Option<T>> {
    let Some(v) = row.get::<_, Option<i64>>(idx)? else {
        return Ok(None);
    };
    Ok(Some(T::try_from(v).unwrap_or_else(|_| {
        let column = row.as_ref().column_name(idx).unwrap_or("?");
        log::warn!("column {column}: {v} is out of range, read as 0");
        T::default()
    })))
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
    fn out_of_range_integers_read_as_zero_not_wrapped() {
        let db = Db::open_in_memory().unwrap();
        let read = |v: Option<i64>| -> (u16, u32, u64, Option<u32>) {
            db.conn()
                .query_row("select ?1, ?1, ?1, ?1", [v], |r| {
                    Ok((
                        int_col(r, 0)?,
                        int_col(r, 1)?,
                        int_col(r, 2)?,
                        opt_int_col(r, 3)?,
                    ))
                })
                .unwrap()
        };
        assert_eq!(read(Some(993)), (993, 993, 993, Some(993)));
        // 70000 used to read back as port 4464.
        assert_eq!(read(Some(70_000)), (0, 70_000, 70_000, Some(70_000)));
        // A negative size used to read back as 1.8e19.
        assert_eq!(read(Some(-1)), (0, 0, 0, Some(0)));
        assert_eq!(read(None).3, None);
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
