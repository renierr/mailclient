//! Schema versioning. v1 creates everything from `schema.sql`.
//!
//! Rule (see AGENT.md): never edit a released migration in place —
//! add a new `migrate_vN` and bump [`SCHEMA_VERSION`].

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

/// Current schema version.
pub const SCHEMA_VERSION: u32 = 12;

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

/// v11 DDL: re-create the FTS update trigger with a WHEN guard, so that
/// updating a flag no longer re-indexes the whole message body. Must drop
/// first: the version in `schema.sql` is `create ... if not exists`, which
/// leaves an existing (unguarded) trigger in place.
const SCHEMA_V11: &str = "drop trigger if exists trg_messages_au;
create trigger trg_messages_au after update on messages
when old.subject is not new.subject
  or old.from_addr is not new.from_addr
  or old.body_text is not new.body_text
begin
    insert into messages_fts (messages_fts, rowid, subject, from_addr, body_text)
    values ('delete', old.id, old.subject, old.from_addr, old.body_text);
    insert into messages_fts (rowid, subject, from_addr, body_text)
    values (new.id, new.subject, new.from_addr, new.body_text);
end;";

/// Run `ALTER TABLE ... ADD COLUMN` statements, tolerating columns that are
/// already there.
///
/// SQLite errors on a duplicate column, and that case is benign here: a fresh
/// install gets every column from `schema.sql` but can still carry an older
/// version stamp, so the catch-up migrations re-add what already exists. Any
/// other error is a real failure and aborts.
fn add_columns(conn: &Connection, statements: &[&str]) -> Result<()> {
    for stmt in statements {
        if let Err(e) = conn.execute_batch(stmt) {
            let msg = e.to_string().to_ascii_lowercase();
            if !(msg.contains("duplicate column") || msg.contains("already exists")) {
                return Err(e.into());
            }
        }
    }
    Ok(())
}

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
        // v3: `messages.flags_dirty`.
        add_columns(conn, &[SCHEMA_V3])?;
    }
    if current < 4 {
        // v4: `attachments.data` BLOB + `is_inline` marker.
        add_columns(
            conn,
            &[
                "alter table attachments add column data blob;",
                "alter table attachments add column is_inline integer not null default 0;",
            ],
        )?;
    }
    if current < 5 {
        // v5: `accounts.from_name` — sender display name (`""` = address only).
        add_columns(
            conn,
            &["alter table accounts add column from_name text not null default '';"],
        )?;
    }
    if current < 6 {
        // v6: `messages.raw_headers` for the technical-headers view.
        add_columns(conn, &["alter table messages add column raw_headers text;"])?;
    }
    if current < 7 {
        // v7: `folders.server_total` — last SELECT's message count.
        add_columns(
            conn,
            &["alter table folders add column server_total integer;"],
        )?;
    }
    if current < 8 {
        // v8: `contacts.alias`, backfilled from the transferred real name.
        add_columns(conn, &["alter table contacts add column alias text;"])?;
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
        // v9: outbox keeps the raw MIME and envelope, so a send survives a crash.
        add_columns(
            conn,
            &[
                "alter table send_queue add column raw_mime blob;",
                "alter table send_queue add column envelope_from text;",
                "alter table send_queue add column envelope_to text not null default '[]';",
            ],
        )?;
    }
    if current < 10 {
        // v10: `folders.highest_modseq` for CONDSTORE / QRESYNC.
        add_columns(conn, &[SCHEMA_V10])?;
    }
    if current < 11 {
        // v11: stop the FTS trigger re-indexing bodies on every flag change.
        conn.execute_batch(SCHEMA_V11)?;
    }
    if current < 12 {
        // v12: folder paths were stored as raw IMAP modified UTF-7
        // (`Entw&APw-rfe`). Discovery now stores decoded Unicode
        // (`Entwürfe`), so rename existing rows — otherwise the next sync
        // would file the same mailbox twice (raw row orphaned, decoded
        // row fresh). Merging into an already-decoded row moves its
        // messages first (conflicting UIDs stay with the survivor).
        if let Err(e) = migrate_folder_paths_utf7(conn) {
            log::warn!("migration v12: folder UTF-7 rename failed: {e}");
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

/// Decode raw IMAP modified-UTF-7 folder paths to Unicode (see v12).
fn migrate_folder_paths_utf7(conn: &Connection) -> Result<()> {
    let rows: Vec<(i64, i64, String)> = {
        let mut stmt =
            conn.prepare("select id, account_id, path from folders where path like '%&%-%'")?;
        let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        mapped.collect::<std::result::Result<Vec<_>, _>>()?
    };
    for (id, account_id, path) in rows {
        let decoded = crate::sync::imap::decode_modified_utf7(&path);
        if decoded == path {
            continue;
        }
        let survivor: Option<i64> = conn
            .query_row(
                "select id from folders where account_id = ?1 and path = ?2",
                rusqlite::params![account_id, decoded],
                |row| row.get(0),
            )
            .optional()?;
        match survivor {
            Some(keep) if keep != id => {
                conn.execute(
                    "delete from messages where folder_id = ?1 and uid in
                     (select uid from messages where folder_id = ?2)",
                    rusqlite::params![id, keep],
                )?;
                conn.execute(
                    "update messages set folder_id = ?1 where folder_id = ?2",
                    rusqlite::params![keep, id],
                )?;
                conn.execute("delete from folders where id = ?1", [id])?;
                log::info!("migration v12: merged folder {path} into existing {decoded}");
            }
            _ => {
                conn.execute(
                    "update folders set path = ?1 where id = ?2",
                    rusqlite::params![decoded, id],
                )?;
                log::info!("migration v12: renamed folder {path} to {decoded}");
            }
        }
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
        assert_eq!(version, SCHEMA_VERSION.to_string());
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
        assert_eq!(version, SCHEMA_VERSION.to_string());
        conn.execute("select highest_modseq from folders", [])
            .unwrap();
    }

    #[test]
    fn v12_migration_decodes_utf7_folder_paths() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '11')",
            [],
        )
        .unwrap();
        conn.execute(
            "insert into accounts (name, email_address, imap_host, smtp_host,
              auth_vault_key, created_at, updated_at)
             values ('t', 't@example.com', 'i', 's', 'k', 't', 't')",
            [],
        )
        .unwrap();
        conn.execute(
            "insert into folders (account_id, path, delimiter, role, created_at, updated_at)
             values (1, '[Google Mail]/Entw&APw-rfe', '/', 'custom', 't', 't')",
            [],
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
        assert_eq!(version, SCHEMA_VERSION.to_string());
        let path: String = conn
            .query_row("select path from folders where account_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(path, "[Google Mail]/Entwürfe");
    }

    #[test]
    fn v11_migration_reguards_the_fts_update_trigger() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '10')",
            [],
        )
        .unwrap();
        // The shape an already-installed database has: no WHEN clause.
        conn.execute_batch(
            "drop trigger trg_messages_au;
             create trigger trg_messages_au after update on messages begin
                 insert into messages_fts (messages_fts, rowid, subject, from_addr, body_text)
                 values ('delete', old.id, old.subject, old.from_addr, old.body_text);
                 insert into messages_fts (rowid, subject, from_addr, body_text)
                 values (new.id, new.subject, new.from_addr, new.body_text);
             end;",
        )
        .unwrap();

        ensure_schema(&conn).unwrap();

        let sql: String = conn
            .query_row(
                "select sql from sqlite_master where type = 'trigger'
                   and name = 'trg_messages_au'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            sql.to_ascii_lowercase().contains("when old.subject"),
            "{sql}"
        );
    }
}
