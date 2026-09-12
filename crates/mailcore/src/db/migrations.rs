//! Schema versioning. v1 creates everything from `schema.sql`.
//!
//! Rule (see AGENT.md): never edit a released migration in place —
//! add a new `migrate_vN` and bump [`SCHEMA_VERSION`].

use rusqlite::Connection;

use crate::error::Result;

/// Current schema version.
pub const SCHEMA_VERSION: u32 = 4;

/// Full DDL for fresh installs (== latest schema).
const SCHEMA_FULL: &str = include_str!("schema.sql");

/// v2 DDL: app settings table (already part of `schema.sql` for fresh installs).
const SCHEMA_V2: &str = "create table if not exists settings (
    key   text primary key,
    value text not null
);";

/// v3 DDL: `messages.flags_dirty` — local read/star changes awaiting an IMAP
/// push, so flag toggles never block the UI on the network.
const SCHEMA_V3: &str = "alter table messages add column flags_dirty integer not null default 0;";

/// v4 DDL: attachment bytes in SQLite (`attachments.data` BLOB) plus an
/// `is_inline` marker, so files travel with the single DB file instead of a
/// sidecar directory. `storage_path` stays for old rows (unused by new code).
const SCHEMA_V4: &str = "alter table attachments add column data blob;
    alter table attachments add column is_inline integer not null default 0;";

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
        conn.execute_batch(SCHEMA_FULL)?;
        conn.execute(
            "insert into schema_meta (key, value) values ('version', ?1)",
            [SCHEMA_VERSION.to_string()],
        )?;
        return Ok(());
    }
    if current < 2 {
        conn.execute_batch(SCHEMA_V2)?;
    }
    if current < 3 {
        conn.execute_batch(SCHEMA_V3)?;
    }
    if current < 4 {
        // `ALTER TABLE ... ADD COLUMN` errors when the column already exists
        // (e.g. a fresh v4 `schema.sql` install that still carries version 3
        // in `schema_meta`); those are benign — anything else aborts.
        for stmt in [
            "alter table attachments add column data blob;",
            "alter table attachments add column is_inline integer not null default 0;",
        ] {
            if let Err(e) = conn.execute_batch(stmt) {
                let msg = e.to_string().to_ascii_lowercase();
                if !(msg.contains("duplicate column") || msg.contains("already exists")) {
                    return Err(e.into());
                }
            }
        }
        let _ = SCHEMA_V4;
    }
    // Future: `if current < 4 { migrate_v4(conn)?; }` etc.
    if current != SCHEMA_VERSION {
        conn.execute(
            "update schema_meta set value = ?1 where key = 'version'",
            [SCHEMA_VERSION.to_string()],
        )?;
    }
    Ok(())
}
