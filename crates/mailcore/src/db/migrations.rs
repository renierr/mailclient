//! Schema versioning. v1 creates everything from `schema.sql`.
//!
//! Rule (see AGENT.md): never edit a released migration in place —
//! add a new `migrate_vN` and bump [`SCHEMA_VERSION`].

use rusqlite::Connection;

use crate::error::Result;

/// Current schema version.
pub const SCHEMA_VERSION: u32 = 1;

/// Embedded v1 DDL.
const SCHEMA_V1: &str = include_str!("schema.sql");

/// Create or upgrade the database to [`SCHEMA_VERSION`].
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    let current: u32 = conn
        .query_row(
            "select value from schema_meta where key = 'version'",
            [],
            |row| {
                let v: String = row.get(0)?;
                Ok(v.parse::<u32>().unwrap_or(0))
            },
        )
        .unwrap_or(0);

    if current == 0 {
        conn.execute_batch(SCHEMA_V1)?;
        conn.execute(
            "insert into schema_meta (key, value) values ('version', ?1)",
            [SCHEMA_VERSION.to_string()],
        )?;
    }
    // Future: `if current < 2 { migrate_v2(conn)?; }` etc.
    Ok(())
}
