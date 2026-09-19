//! Schema versioning. v1 creates everything from `schema.sql`.
//!
//! Rule (see AGENT.md): never edit a released migration in place —
//! add a new `migrate_vN` and bump [`SCHEMA_VERSION`].

use rusqlite::Connection;

use crate::error::Result;

/// Current schema version.
pub const SCHEMA_VERSION: u32 = 10;

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

/// v10 DDL: `folders.highest_modseq` — CONDSTORE / QRESYNC modseq tracking per folder.
const SCHEMA_V10: &str =
    "alter table folders add column highest_modseq integer not null default 0;";

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
        // v4 DDL: `attachments.data` BLOB + `is_inline` marker.
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
    }
    if current < 5 {
        // `accounts.from_name`: sender display name (`""` = address only).
        // Same benign-duplicate tolerance as v4 (fresh v5 `schema.sql`
        // installs carrying an older version stamp).
        if let Err(e) = conn
            .execute_batch("alter table accounts add column from_name text not null default '';")
        {
            let msg = e.to_string().to_ascii_lowercase();
            if !(msg.contains("duplicate column") || msg.contains("already exists")) {
                return Err(e.into());
            }
        }
    }
    if current < 6 {
        if let Err(e) = conn.execute_batch("alter table messages add column raw_headers text;") {
            let msg = e.to_string().to_ascii_lowercase();
            if !(msg.contains("duplicate column") || msg.contains("already exists")) {
                return Err(e.into());
            }
        }
    }
    if current < 7 {
        if let Err(e) = conn.execute_batch("alter table folders add column server_total integer;") {
            let msg = e.to_string().to_ascii_lowercase();
            if !(msg.contains("duplicate column") || msg.contains("already exists")) {
                return Err(e.into());
            }
        }
    }
    if current < 8 {
        if let Err(e) = conn.execute_batch("alter table contacts add column alias text;") {
            let msg = e.to_string().to_ascii_lowercase();
            if !(msg.contains("duplicate column") || msg.contains("already exists")) {
                return Err(e.into());
            }
        }
        if let Err(e) = conn.execute_batch(
            "update contacts set alias = name where alias is null and name is not null;",
        ) {
            log::warn!("migration v8: alias backfill failed: {e}");
        }
        if let Err(e) = crate::store::contacts::seed_contacts_from_connection(conn) {
            log::warn!("migration v8: contact seeding failed: {e}");
        }
    }
    if current < 9 {
        for stmt in [
            "alter table send_queue add column raw_mime blob;",
            "alter table send_queue add column envelope_from text;",
            "alter table send_queue add column envelope_to text not null default '[]';",
        ] {
            if let Err(e) = conn.execute_batch(stmt) {
                let msg = e.to_string().to_ascii_lowercase();
                if !(msg.contains("duplicate column") || msg.contains("already exists")) {
                    return Err(e.into());
                }
            }
        }
    }
    if current < 10 {
        if let Err(e) = conn.execute_batch(SCHEMA_V10) {
            let msg = e.to_string().to_ascii_lowercase();
            if !(msg.contains("duplicate column") || msg.contains("already exists")) {
                return Err(e.into());
            }
        }
    }
    if current != SCHEMA_VERSION {
        conn.execute(
            "update schema_meta set value = ?1 where key = 'version'",
            [SCHEMA_VERSION.to_string()],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v8_migration_adds_alias_column_and_updates_version() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '7')",
            [],
        )
        .unwrap();
        conn.execute_batch("alter table contacts drop column alias;")
            .unwrap();

        ensure_schema(&conn).unwrap();

        let version: String = conn
            .query_row(
                "select value from schema_meta where key = 'version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION.to_string());

        conn.execute("select alias from contacts", []).unwrap();
    }

    #[test]
    fn v9_migration_adds_outbox_mime_columns() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '8')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "alter table send_queue drop column raw_mime;
             alter table send_queue drop column envelope_from;
             alter table send_queue drop column envelope_to;",
        )
        .unwrap();

        ensure_schema(&conn).unwrap();

        let version: String = conn
            .query_row(
                "select value from schema_meta where key = 'version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, "10");
        conn.execute(
            "select raw_mime, envelope_from, envelope_to from send_queue",
            [],
        )
        .unwrap();
    }

    #[test]
    fn v10_migration_adds_highest_modseq_column() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '9')",
            [],
        )
        .unwrap();
        conn.execute_batch("alter table folders drop column highest_modseq;")
            .unwrap();

        ensure_schema(&conn).unwrap();

        let version: String = conn
            .query_row(
                "select value from schema_meta where key = 'version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, "10");
        conn.execute("select highest_modseq from folders", [])
            .unwrap();
    }
}
