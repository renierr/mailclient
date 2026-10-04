//! Schema versioning. v1 creates everything from `schema.sql`.
//!
//! Rule (see AGENT.md): never edit a released migration in place —
//! add a new `migrate_vN` and bump [`SCHEMA_VERSION`].

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

/// Current schema version.
pub const SCHEMA_VERSION: u32 = 21;

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

/// v15 DDL: `pending_moves` — undoable moves wait here until pushed.
const SCHEMA_V15: &str = "create table if not exists pending_moves (
    message_id     integer primary key references messages (id) on delete cascade,
    batch          text not null,
    action         text not null,
    dest_folder_id integer references folders (id) on delete cascade,
    due_at         text not null,
    attempts       integer not null default 0,
    created_at     text not null,
    updated_at     text not null
);
create index if not exists idx_pending_moves_batch on pending_moves (batch);
create index if not exists idx_pending_moves_due on pending_moves (due_at);";

/// v17 DDL: `account_settings` — per-account overrides of app settings.
const SCHEMA_V17: &str = "create table if not exists account_settings (
    account_id integer not null references accounts (id) on delete cascade,
    key        text not null,
    value      text not null,
    created_at text not null,
    updated_at text not null,
    primary key (account_id, key)
);";

/// v19 DDL: the search index also covers the sender name and the To/Cc/Bcc
/// addresses. FTS5 cannot add columns, so the table and its triggers are
/// dropped, re-created as in `schema.sql`, and rebuilt from `messages`.
const SCHEMA_V19: &str = "drop trigger if exists trg_messages_ai;
drop trigger if exists trg_messages_ad;
drop trigger if exists trg_messages_au;
drop table if exists messages_fts;
create virtual table messages_fts using fts5 (
    subject,
    from_addr,
    from_name,
    to_addrs,
    cc_addrs,
    bcc_addrs,
    body_text,
    content = 'messages',
    content_rowid = 'id'
);
create trigger trg_messages_ai after insert on messages begin
    insert into messages_fts (rowid, subject, from_addr, from_name, to_addrs, cc_addrs, bcc_addrs, body_text)
    values (new.id, new.subject, new.from_addr, new.from_name, new.to_addrs, new.cc_addrs, new.bcc_addrs, new.body_text);
end;
create trigger trg_messages_ad after delete on messages begin
    insert into messages_fts (messages_fts, rowid, subject, from_addr, from_name, to_addrs, cc_addrs, bcc_addrs, body_text)
    values ('delete', old.id, old.subject, old.from_addr, old.from_name, old.to_addrs, old.cc_addrs, old.bcc_addrs, old.body_text);
end;
create trigger trg_messages_au after update on messages
when old.subject is not new.subject
  or old.from_addr is not new.from_addr
  or old.from_name is not new.from_name
  or old.to_addrs is not new.to_addrs
  or old.cc_addrs is not new.cc_addrs
  or old.bcc_addrs is not new.bcc_addrs
  or old.body_text is not new.body_text
begin
    insert into messages_fts (messages_fts, rowid, subject, from_addr, from_name, to_addrs, cc_addrs, bcc_addrs, body_text)
    values ('delete', old.id, old.subject, old.from_addr, old.from_name, old.to_addrs, old.cc_addrs, old.bcc_addrs, old.body_text);
    insert into messages_fts (rowid, subject, from_addr, from_name, to_addrs, cc_addrs, bcc_addrs, body_text)
    values (new.id, new.subject, new.from_addr, new.from_name, new.to_addrs, new.cc_addrs, new.bcc_addrs, new.body_text);
end;
insert into messages_fts (messages_fts) values ('rebuild');";

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
    if current < 13 {
        // v13: the address index ignores case, so `User@x` cannot be added
        // next to `user@x`. Existing rows that already differ only by case
        // would make the unique index fail; keep the old one then.
        match conn.execute_batch(
            "create unique index if not exists idx_accounts_email_nocase
                 on accounts (email_address collate nocase);",
        ) {
            Ok(()) => {
                conn.execute_batch("drop index if exists idx_accounts_email;")?;
            }
            Err(e) => log::warn!("migration v13: accounts differ only by case, kept index: {e}"),
        }
    }
    if current < 14 {
        // v14: plain-text mail was stored with a `body_html` that mail-parser
        // generated from the text, so it rendered as HTML. Drop exactly those.
        if let Err(e) = drop_generated_html(conn) {
            log::warn!("migration v14: generated html cleanup failed: {e}");
        }
    }
    if current < 15 {
        // v15: `pending_moves`, the grace-period queue behind Undo.
        conn.execute_batch(SCHEMA_V15)?;
    }
    if current < 16 {
        // v16: `messages.from_name` — sender display name for the list rows.
        // Stored at sync time from now on; existing rows backfill from
        // their stored headers (empty stays empty = address only).
        add_columns(conn, &["alter table messages add column from_name text;"])?;
        if let Err(e) = backfill_from_names(conn) {
            log::warn!("migration v16: sender-name backfill failed: {e}");
        }
    }
    if current < 17 {
        // v17: `account_settings`, per-account overrides of sync settings.
        conn.execute_batch(SCHEMA_V17)?;
    }
    if current < 18 {
        // v18: SMTP used to demand STARTTLS even with security `none`, which
        // older Flutter forms offered without a warning. SMTP now honours
        // `none`, so such accounts would silently start sending in the
        // clear. Reset them to STARTTLS; a real plaintext opt-in is made
        // again in the account form, which warns about it.
        let reset = conn.execute(
            "update accounts set smtp_security = 'starttls', updated_at = ?1
             where lower(trim(smtp_security)) in ('none', 'plain')",
            [crate::store::now()],
        )?;
        if reset > 0 {
            log::info!("migration v18: {reset} account(s) reset from plaintext SMTP to STARTTLS");
        }
    }
    if current < 19 {
        // v19: sender name and recipients join the search index.
        conn.execute_batch(SCHEMA_V19)?;
    }
    if current < 20 {
        // v20: no DDL — repairs rows. Senders (newsletters) declare body
        // images as `Content-Disposition: attachment` with a `Content-ID`
        // the HTML shows via `cid:`; those listed as files and raised the
        // list icon. Mark them inline and clear flags left without a real
        // file, and fix stored MIME types against magic bytes. New mail is
        // parsed this way from now on (see `parse_to_new`).
        if let Err(e) = migrate_cid_inline_attachments(conn) {
            log::warn!("migration v20: inline attachment repair failed: {e}");
        }
    }
    if current < 21 {
        // v21: `contacts.sent_count` — how often mail was sent *to* an
        // address, so people you wrote to rank above merely harvested ones
        // and are never cleanup `stale` candidates. Backfilled from cached
        // Sent mail; only existing contacts are credited, removed ones stay
        // forgotten.
        add_columns(
            conn,
            &["alter table contacts add column sent_count integer not null default 0;"],
        )?;
        if let Err(e) = crate::store::contacts::backfill_sent_counts_from_connection(conn) {
            log::warn!("migration v21: sent-count backfill failed: {e}");
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

/// Fill `messages.from_name` from stored header blocks (see v16). Rows
/// without usable headers keep NULL, and the list shows their address.
fn backfill_from_names(conn: &Connection) -> Result<()> {
    let rows: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "select id, raw_headers from messages
              where raw_headers is not null and raw_headers != ''
                and (from_name is null or from_name = '')",
        )?;
        let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        mapped.collect::<std::result::Result<Vec<_>, _>>()?
    };
    let mut filled = 0;
    let tx = conn.unchecked_transaction()?;
    for (id, raw) in &rows {
        if let Some(name) = crate::store::messages::display_name_from_headers(raw) {
            tx.execute(
                "update messages set from_name = ?1 where id = ?2",
                rusqlite::params![name, id],
            )?;
            filled += 1;
        }
    }
    tx.commit()?;
    log::info!("migration v16: sender name backfilled for {filled} messages");
    Ok(())
}

/// Mark `cid:`-shown body images inline and repair stored MIME types (see
/// v20). Only messages whose body mentions `cid:` and still list a
/// non-inline `Content-ID` part are touched; the magic check reads just
/// the first 12 bytes (`substr`), never whole BLOBs.
fn migrate_cid_inline_attachments(conn: &Connection) -> Result<()> {
    let msgs: Vec<(i64, String)> = {
        let mut stmt = conn.prepare(
            "select m.id, m.body_html from messages m
              where m.body_html like '%cid:%'
                and exists (select 1 from attachments a
                            where a.message_id = m.id and a.is_inline = 0
                              and a.content_id is not null
                              and trim(a.content_id) != '')",
        )?;
        let mapped = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        mapped.collect::<std::result::Result<Vec<_>, _>>()?
    };
    let (mut marked, mut cleared) = (0, 0);
    for (id, html) in &msgs {
        let parts: Vec<(i64, Option<String>)> = conn
            .prepare(
                "select id, content_id from attachments
                  where message_id = ?1 and is_inline = 0",
            )?
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (aid, cid) in &parts {
            if crate::html::is_body_referenced(cid.as_deref(), Some(html)) {
                conn.execute("update attachments set is_inline = 1 where id = ?1", [aid])?;
                marked += 1;
            }
        }
        let remaining: i64 = conn.query_row(
            "select count(*) from attachments where message_id = ?1 and is_inline = 0",
            [id],
            |row| row.get(0),
        )?;
        if remaining == 0 {
            conn.execute(
                "update messages set has_attachments = 0 where id = ?1",
                [id],
            )?;
            cleared += 1;
        }
    }
    let mut fixed = 0;
    let blobs: Vec<(i64, Option<String>, Vec<u8>)> = conn
        .prepare(
            "select id, mime_type, substr(data, 1, 12) from attachments
              where data is not null",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (aid, mime, head) in &blobs {
        if let Some(better) = crate::mime::corrected_mime(mime.as_deref(), head) {
            conn.execute(
                "update attachments set mime_type = ?1 where id = ?2",
                rusqlite::params![better, aid],
            )?;
            fixed += 1;
        }
    }
    log::info!(
        "migration v20: {marked} cid: part(s) marked inline, \
         {fixed} mime type(s) fixed, {cleared} flag(s) cleared"
    );
    Ok(())
}

/// Null `body_html` where it is only mail-parser's conversion of `body_text`
/// (see v14). Compared byte for byte, so a real HTML part is never touched.
fn drop_generated_html(conn: &Connection) -> Result<()> {
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare(
            "select id, coalesce(body_text, ''), body_html from messages
             where body_html is not null",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut ids = Vec::new();
        for row in rows {
            let (id, text, html) = row?;
            if html == mail_parser::decoders::html::text_to_html(&text) {
                ids.push(id);
            }
        }
        ids
    };
    let tx = conn.unchecked_transaction()?;
    for id in &ids {
        tx.execute("update messages set body_html = null where id = ?1", [id])?;
    }
    tx.commit()?;
    log::info!(
        "migration v14: {} plain-text bodies no longer treated as html",
        ids.len()
    );
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
    fn v14_migration_drops_only_generated_html() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '13')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "insert into accounts (id, name, email_address, imap_host, imap_port,
                 imap_security, imap_username, smtp_host, smtp_port,
                 smtp_security, smtp_username, auth_vault_key, created_at, updated_at)
             values (1, 'n', 'a@example.com', 'h', 993, 'tls', 'u', 'h', 465, 'tls', 'u', 'k', 't', 't');
             insert into folders (id, account_id, path, delimiter, role, created_at, updated_at)
             values (1, 1, 'INBOX', '/', 'inbox', 't', 't');",
        )
        .unwrap();
        let add = |uid: i64, text: &str, html: &str| {
            conn.execute(
                "insert into messages (account_id, folder_id, uid, body_text, body_html,
                     created_at, updated_at)
                 values (1, 1, ?1, ?2, ?3, 't', 't')",
                rusqlite::params![uid, text, html],
            )
            .unwrap();
        };
        add(
            1,
            "a
b<c",
            "<html><body>a<br/>b&lt;c</body></html>",
        );
        add(2, "a", "<p>a</p>");

        ensure_schema(&conn).unwrap();

        let html = |uid: i64| -> Option<String> {
            conn.query_row(
                "select body_html from messages where uid = ?1",
                [uid],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(html(1), None);
        assert_eq!(html(2).as_deref(), Some("<p>a</p>"));
    }

    #[test]
    fn v16_migration_backfills_sender_names_decoded() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '15')",
            [],
        )
        .unwrap();
        // Pre-v19 triggers did not index `from_name`; today's would block the drop.
        conn.execute_batch(
            "drop trigger trg_messages_ai;
             drop trigger trg_messages_ad;
             drop trigger trg_messages_au;
             alter table messages drop column from_name;
             insert into accounts (id, name, email_address, imap_host, imap_port,
                  imap_security, imap_username, smtp_host, smtp_port,
                  smtp_security, smtp_username, auth_vault_key, created_at, updated_at)
              values (1, 'n', 'a@example.com', 'h', 993, 'tls', 'u', 'h', 465, 'tls', 'u', 'k', 't', 't');
             insert into folders (id, account_id, path, delimiter, role, created_at, updated_at)
              values (1, 1, 'INBOX', '/', 'inbox', 't', 't');",
        )
        .unwrap();
        let add = |uid: i64, raw: &str| {
            conn.execute(
                "insert into messages (account_id, folder_id, uid, raw_headers,
                     created_at, updated_at)
                  values (1, 1, ?1, ?2, 't', 't')",
                rusqlite::params![uid, raw],
            )
            .unwrap();
        };
        add(
            1,
            "From: =?UTF-8?Q?J=C3=BCrgen_M=C3=BCller?= <juergen@example.com>\r\nSubject: hi",
        );
        add(2, "From: plain@example.com\r\nSubject: hi");
        add(3, "Subject: no sender");

        ensure_schema(&conn).unwrap();

        let name = |uid: i64| -> Option<String> {
            conn.query_row(
                "select from_name from messages where uid = ?1",
                [uid],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(name(1).as_deref(), Some("Jürgen Müller"));
        assert_eq!(name(2), None);
        assert_eq!(name(3), None);
    }

    #[test]
    fn v13_migration_makes_the_address_index_case_insensitive() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '12')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "drop index idx_accounts_email_nocase;
             create unique index idx_accounts_email on accounts (email_address);",
        )
        .unwrap();

        ensure_schema(&conn).unwrap();

        let insert = |email: &str| {
            conn.execute(
                "insert into accounts (name, email_address, imap_host, imap_port,
                     imap_security, imap_username, smtp_host, smtp_port,
                     smtp_security, smtp_username, auth_vault_key, created_at, updated_at)
                 values ('n', ?1, 'h', 993, 'tls', 'u', 'h', 465, 'tls', 'u', 'k', 't', 't')",
                [email],
            )
        };
        insert("user@example.com").unwrap();
        assert!(insert("User@Example.com").is_err());
        let old: i64 = conn
            .query_row(
                "select count(*) from sqlite_master where name = 'idx_accounts_email'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old, 0);
    }

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

    #[test]
    fn v19_migration_rebuilds_the_index_with_names_and_recipients() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '18')",
            [],
        )
        .unwrap();
        // The v18 index: subject, address and body only.
        conn.execute_batch(
            "drop trigger trg_messages_ai;
             drop trigger trg_messages_ad;
             drop trigger trg_messages_au;
             drop table messages_fts;
             create virtual table messages_fts using fts5 (
                 subject, from_addr, body_text,
                 content = 'messages', content_rowid = 'id');
             insert into accounts (id, name, email_address, imap_host, smtp_host,
                 auth_vault_key, created_at, updated_at)
             values (1, 'a', 'a@example.com', 'i', 's', 'k', 't', 't');
             insert into folders (id, account_id, path, delimiter, role, created_at, updated_at)
             values (1, 1, 'INBOX', '/', 'inbox', 't', 't');
             insert into messages (id, account_id, folder_id, uid, from_name, cc_addrs,
                 created_at, updated_at)
             values (1, 1, 1, 1, 'Anna', '[\"carl@example.org\"]', 't', 't');",
        )
        .unwrap();

        ensure_schema(&conn).unwrap();

        let fresh = rusqlite::Connection::open_in_memory().unwrap();
        fresh.execute_batch(SCHEMA_FULL).unwrap();
        let shape = |c: &Connection| -> Vec<String> {
            c.prepare(
                "select sql from sqlite_master where name like '%messages_fts'
                 or name like 'trg_messages_a_' order by name",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
        };
        let norm = |v: Vec<String>| -> Vec<String> {
            v.into_iter()
                .map(|s| s.replace("if not exists ", ""))
                .collect()
        };
        assert_eq!(norm(shape(&conn)), norm(shape(&fresh)));
        // Existing rows were re-indexed, new columns included.
        for q in ["from_name : anna", "cc_addrs : carl"] {
            let n: i64 = conn
                .query_row(
                    "select count(*) from messages_fts where messages_fts match ?1",
                    [q],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{q}");
        }
    }

    #[test]
    fn v20_migration_marks_cid_shown_parts_inline_and_fixes_mime() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '19')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "insert into accounts (id, name, email_address, imap_host, smtp_host,
                 auth_vault_key, created_at, updated_at)
             values (1, 'a', 'a@example.com', 'i', 's', 'k', 't', 't');
             insert into folders (id, account_id, path, delimiter, role, created_at, updated_at)
             values (1, 1, 'INBOX', '/', 'inbox', 't', 't');
             insert into messages (id, account_id, folder_id, uid, body_html,
                 has_attachments, created_at, updated_at)
             values (1, 1, 1, 1, '<p><img src=\"cid:yellowLogo\"></p>', 1, 't', 't'),
                    (2, 1, 1, 2, '<p>no images</p>', 1, 't', 't');
             insert into attachments (id, message_id, filename, mime_type, size,
                 content_id, data, is_inline, created_at)
             values (1, 1, 'inline', 'application/octet-stream', 10,
                 'yellowLogo', X'89504E470D0A1A0A7878', 0, 't'),
                    (2, 2, 'a.pdf', 'application/pdf', 4, null, null, 0, 't');",
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
        // Body-shown part: inline, MIME fixed from magic, flag cleared.
        let logo: (i64, String, i64) = conn
            .query_row(
                "select is_inline, mime_type,
                    (select has_attachments from messages where id = 1)
                 from attachments where id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(logo, (1, "image/png".to_string(), 0));
        // A real file is untouched, and its flag stays raised.
        let file: (i64, i64) = conn
            .query_row(
                "select is_inline,
                    (select has_attachments from messages where id = 2)
                 from attachments where id = 2",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(file, (0, 1));
    }

    #[test]
    fn v17_migration_adds_account_settings() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '16')",
            [],
        )
        .unwrap();
        conn.execute_batch("drop table account_settings;").unwrap();

        ensure_schema(&conn).unwrap();

        conn.execute(
            "select account_id, key, value, created_at, updated_at from account_settings",
            [],
        )
        .unwrap();
    }

    #[test]
    fn v18_migration_resets_plaintext_smtp_to_starttls() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_FULL).unwrap();
        conn.execute(
            "insert into schema_meta (key, value) values ('version', '17')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "insert into accounts (id, name, email_address, imap_host, imap_port,
                 imap_security, imap_username, smtp_host, smtp_port,
                 smtp_security, smtp_username, auth_vault_key, created_at, updated_at)
             values (1, 'a', 'a@example.com', 'h', 143, 'none', 'u', 'h', 25, 'none', 'u', 'k', 't', 't'),
                    (2, 'b', 'b@example.com', 'h', 993, 'tls', 'u', 'h', 25, ' Plain ', 'u', 'k', 't', 't'),
                    (3, 'c', 'c@example.com', 'h', 993, 'tls', 'u', 'h', 465, 'tls', 'u', 'k', 't', 't');",
        )
        .unwrap();

        ensure_schema(&conn).unwrap();

        let row = |id: i64| -> (String, String, String) {
            conn.query_row(
                "select imap_security, smtp_security, updated_at from accounts where id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        let (imap, smtp, updated) = row(1);
        assert_eq!((imap.as_str(), smtp.as_str()), ("none", "starttls"));
        assert_ne!(updated, "t");
        assert_eq!(row(2).1, "starttls");
        assert_eq!(row(3), ("tls".into(), "tls".into(), "t".into()));
    }
}
