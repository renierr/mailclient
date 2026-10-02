//! Local storage maintenance: stats, temp cleanup, database export and
//! cache trimming. Both frontends show this as a settings section; every
//! decision lives here.
//!
//! Everything in this module is local-only: trimming the message cache or
//! evicting attachment bytes never talks to the mail server, so nothing
//! stored there can be harmed. Trimmed messages come back with the next sync
//! while they are inside the sync window, and evicted attachment bytes
//! re-download on the next explicit open or save through the normal on-demand
//! path (`attachment_has_data` is false again, so the adapters fetch first).

use std::path::{Path, PathBuf};

use rusqlite::params;
use serde::Serialize;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::paths::{file_url_to_path, free_path, prune_stale_draft_dirs, DRAFT_TEMP_PREFIX};

/// Newest-N messages per folder kept by [`trim_local_cache`]: the same
/// window a full folder sync fetches, so trimming converges the cache to
/// what sync keeps anyway.
pub use crate::sync::imap::FULL_SYNC_WINDOW as TRIM_KEEP_PER_FOLDER;

/// `1023` → `"1023 B"`, `2048` → `"2 KB"`, `3 MiB` → `"3.0 MB"`. One
/// definition so both frontends report the same sizes.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{} KB", bytes / KB)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    }
}

/// What the maintenance section shows. Raw byte counts for logic plus
/// preformatted `*_display` strings, so QML and Dart render sizes without
/// a second formatting twin (cf. `mailcore::badge`: the core says what to
/// show, the frontend only how).
#[derive(Debug, Clone, Serialize)]
pub struct StorageStats {
    pub db_path: String,
    pub db_bytes: u64,
    pub db_display: String,
    pub message_count: u64,
    pub cached_files: u64,
    pub cached_bytes: u64,
    pub cached_display: String,
    pub temp_files: u64,
    pub temp_bytes: u64,
    pub temp_display: String,
    pub temp_draft_dirs: u64,
    pub keep_per_folder: usize,
}

/// Collect [`StorageStats`]. `db_path` is the live database file (the
/// bridges know it; `Db` does not carry its path). `temp_dir` is the
/// `mailclient-attachments` folder the calling frontend stages viewer
/// copies into — Qt's `temp_dir()` differs from Flutter's cache dir, so it
/// cannot be guessed here. Missing files count as zero, never as an error.
pub fn storage_stats(db: &Db, db_path: &Path, temp_dir: &Path) -> Result<StorageStats> {
    let message_count: i64 = db
        .conn()
        .query_row("select count(*) from messages", [], |r| r.get(0))?;
    let (cached_files, cached_bytes): (i64, Option<i64>) = db.conn().query_row(
        &format!("select count(*), sum(length(data)) from attachments where {EVICTABLE}"),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (temp_files, temp_bytes) = dir_file_totals(temp_dir);
    let temp_draft_dirs = draft_dir_count(temp_dir.parent());
    let db_bytes = db_file_bytes(db_path);
    Ok(StorageStats {
        db_path: db_path.to_string_lossy().into_owned(),
        db_bytes,
        db_display: format_bytes(db_bytes),
        message_count: message_count.max(0) as u64,
        cached_files: cached_files.max(0) as u64,
        cached_bytes: cached_bytes.unwrap_or(0).max(0) as u64,
        cached_display: format_bytes(cached_bytes.unwrap_or(0).max(0) as u64),
        temp_files,
        temp_bytes,
        temp_display: format_bytes(temp_bytes),
        temp_draft_dirs,
        keep_per_folder: TRIM_KEEP_PER_FOLDER,
    })
}

/// [`storage_stats`] serialised for the feeds / FFI (one definition, like
/// every other `*_json` the two frontends share).
pub fn storage_stats_json(db: &Db, db_path: &Path, temp_dir: &Path) -> Result<String> {
    Ok(serde_json::to_string(&storage_stats(
        db, db_path, temp_dir,
    )?)?)
}

/// Main file plus `-wal`/`-shm`: a live database's size is all three.
fn db_file_bytes(db_path: &Path) -> u64 {
    let mut total = 0;
    total += file_bytes(db_path);
    for suffix in ["-wal", "-shm"] {
        let mut name = db_path.as_os_str().to_owned();
        name.push(suffix);
        total += file_bytes(Path::new(&name));
    }
    total
}

fn file_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// `(file count, byte sum)` of plain files directly inside `dir`.
fn dir_file_totals(dir: &Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return (0, 0),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        files += 1;
        bytes += file_bytes(&path);
    }
    (files, bytes)
}

/// Stale or not, how many `mailclient-draft-*` staging dirs sit in `base`.
fn draft_dir_count(base: Option<&Path>) -> u64 {
    let Some(base) = base else { return 0 };
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    // `base` is the system temp folder, often tens of thousands of entries:
    // match the name first and take the type from the directory listing,
    // so only our own entries cost anything.
    entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(DRAFT_TEMP_PREFIX)
                && e.file_type().is_ok_and(|t| t.is_dir())
        })
        .count() as u64
}

/// What [`cleanup_temp_files`] removed.
#[derive(Debug, Clone, Serialize)]
pub struct TempCleanup {
    pub files_removed: u64,
    pub bytes_freed: u64,
    pub bytes_display: String,
    pub draft_dirs_removed: u64,
}

/// Delete every viewer copy in `temp_dir` plus stale draft staging dirs
/// next to it, and report what went away. Best-effort per file: a copy
/// currently open in an external viewer (Windows locks it) is skipped, not
/// fatal.
///
/// Viewer copies are disposable by construction — they are re-written from
/// the cached bytes on every open — so unlike [`trim_local_cache`] there is
/// nothing to keep back.
pub fn cleanup_temp_files(temp_dir: &Path) -> Result<TempCleanup> {
    let mut files_removed = 0;
    let mut bytes_freed = 0;
    if let Ok(entries) = std::fs::read_dir(temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let size = file_bytes(&path);
            if std::fs::remove_file(&path).is_ok() {
                files_removed += 1;
                bytes_freed += size;
            }
        }
    }
    let draft_dirs_removed = remove_stale_draft_dirs(temp_dir.parent());
    Ok(TempCleanup {
        files_removed,
        bytes_freed,
        bytes_display: format_bytes(bytes_freed),
        draft_dirs_removed,
    })
}

fn remove_stale_draft_dirs(base: Option<&Path>) -> u64 {
    let Some(base) = base else { return 0 };
    let before = draft_dir_count(Some(base));
    prune_stale_draft_dirs(base);
    before.saturating_sub(draft_dir_count(Some(base)))
}

/// Write a consistent snapshot of the live database to `dest` (a save-dialog
/// `file://` URL or plain path; a directory gets `mailclient-backup.sqlite`
/// with a free number). Returns where it went.
///
/// Uses `VACUUM INTO`, not a file copy: the live database runs in WAL mode,
/// so the main file alone is not a consistent snapshot, and `VACUUM INTO`
/// compacts at the same time. `VACUUM INTO` takes a literal, not a bound
/// parameter, hence the quote-doubling below.
pub fn export_database(db: &Db, dest: &str) -> Result<PathBuf> {
    let trimmed = dest.trim();
    if trimmed.is_empty() {
        return Err(StoreError::InvalidInput("choose where to save".into()));
    }
    let mut path = file_url_to_path(trimmed);
    if path.is_dir() || trimmed.ends_with('/') || trimmed.ends_with('\\') {
        path.push("mailclient-backup.sqlite");
    }
    let (parent, name) = match (path.parent(), path.file_name()) {
        (Some(p), Some(n)) => {
            let parent = if p.as_os_str().is_empty() {
                Path::new(".").to_path_buf()
            } else {
                p.to_path_buf()
            };
            (parent, n.to_string_lossy().into_owned())
        }
        _ => {
            return Err(StoreError::InvalidInput("choose where to save".into()));
        }
    };
    std::fs::create_dir_all(&parent)?;
    let final_path = free_path(&parent, &name);
    let literal = final_path.to_string_lossy().replace('\'', "''");
    db.conn()
        .execute_batch(&format!("vacuum into '{literal}'"))?;
    Ok(final_path)
}

/// Delete cached messages past the newest [`TRIM_KEEP_PER_FOLDER`] per
/// folder and return how many rows went away. Attachments cascade, the FTS
/// index follows via its delete trigger — and the server is never
/// contacted: rows still on the server come back with the next sync while
/// they are inside the sync window.
///
/// The following are never trimmed, even when old:
/// - drafts (`is_draft`): user content that may live only in the cache;
/// - rows with unpushed flag changes (`flags_dirty`): deleting them would
///   silently drop the change;
/// - rows with a pending undoable move (`pending_moves`): deleting them
///   would silently cancel the undo and its server push;
/// - rows a queued, sending or failed send still references (`send_queue`).
pub fn trim_local_cache(db: &Db, keep_per_folder: usize) -> Result<u64> {
    let folders: Vec<i64> = db
        .conn()
        .prepare("select id from folders")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut removed = 0;
    for folder_id in folders {
        removed += db.conn().execute(
            "delete from messages
              where folder_id = ?1
                and is_draft = 0
                and flags_dirty = 0
                and id not in (
                    select id from messages
                     where folder_id = ?1
                     order by uid desc, id desc
                     limit ?2
                )
                and id not in (select message_id from pending_moves)
                and id not in (
                    select message_id from send_queue
                     where status in ('queued', 'sending', 'failed')
                       and message_id is not null
                )",
            params![folder_id, keep_per_folder as i64],
        )? as u64;
    }
    Ok(removed)
}

/// What [`evict_cached_attachments`] cleared.
#[derive(Debug, Clone, Serialize)]
pub struct AttachmentEviction {
    pub files: u64,
    pub bytes_freed: u64,
    pub bytes_display: String,
}

/// Attachment rows whose bytes the cache may drop. Inline (`cid:`) images
/// are kept: they arrive with the body, and the reader, reply quoting and
/// re-saved drafts read them locally without an on-demand fetch, so
/// evicting them would lose them for good (and strip them from a draft the
/// next save replaces on the server).
const EVICTABLE: &str = "data is not null and is_inline = 0";

/// Drop cached attachment *bytes* while keeping names, sizes and MIME types.
/// Returns what went away. Yes, they come back: clearing `data` makes
/// `attachment_has_data` false again, so the next open or save takes the
/// normal on-demand download path and `fetch_attachments` rewrites the rows.
pub fn evict_cached_attachments(db: &Db) -> Result<AttachmentEviction> {
    let (files, bytes): (i64, Option<i64>) = db.conn().query_row(
        &format!("select count(*), sum(length(data)) from attachments where {EVICTABLE}"),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    db.conn().execute(
        &format!("update attachments set data = null where {EVICTABLE}"),
        [],
    )?;
    let bytes = bytes.unwrap_or(0).max(0) as u64;
    Ok(AttachmentEviction {
        files: files.max(0) as u64,
        bytes_freed: bytes,
        bytes_display: format_bytes(bytes),
    })
}

/// Status-line wording, shared so both frontends report the same text.
pub fn trim_status(removed: u64, keep_per_folder: usize) -> String {
    if removed == 0 {
        format!("Cache is already trim: newest {keep_per_folder} per folder kept")
    } else if removed == 1 {
        format!("Removed 1 older message (newest {keep_per_folder} per folder kept)")
    } else {
        format!("Removed {removed} older messages (newest {keep_per_folder} per folder kept)")
    }
}

/// Status-line wording, shared so both frontends report the same text.
pub fn evict_status(evicted: &AttachmentEviction) -> String {
    if evicted.files == 0 {
        "No downloaded files to remove".to_string()
    } else {
        format!(
            "Removed {} downloaded file(s) ({}) — they download again on open",
            evicted.files, evicted.bytes_display
        )
    }
}

/// Status-line wording, shared so both frontends report the same text.
pub fn cleanup_status(cleanup: &TempCleanup) -> String {
    if cleanup.files_removed == 0 && cleanup.draft_dirs_removed == 0 {
        "No temporary files to remove".to_string()
    } else {
        format!(
            "Removed {} temporary file(s) ({})",
            cleanup.files_removed, cleanup.bytes_display
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount, NewAttachment};
    use crate::store::{accounts, folders, messages};

    fn setup() -> (Db, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "a".into(),
                email_address: "a@example.com".into(),
                from_name: String::new(),
                imap_host: "h".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                imap_username: "u".into(),
                smtp_host: "h".into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                smtp_username: "u".into(),
                auth_vault_key: "k".into(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let f = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        (db, f)
    }

    fn add_message(db: &Db, folder: i64, uid: u32) -> i64 {
        let acc = folders::get(db, folder).unwrap().account_id;
        let mut m = messages::sample_new(acc, folder, uid);
        m.subject = Some(format!("mail {uid}"));
        messages::upsert(db, &m).unwrap()
    }

    fn attach(db: &Db, mid: i64, name: &str) -> i64 {
        attach_part(db, mid, name, false)
    }

    fn attach_part(db: &Db, mid: i64, name: &str, is_inline: bool) -> i64 {
        messages::add_attachment(
            db,
            mid,
            &NewAttachment {
                filename: Some(name.into()),
                mime_type: Some("application/octet-stream".into()),
                size: 3,
                content_id: is_inline.then(|| format!("{name}@example.com")),
                is_inline,
                data: Some(b"abc".to_vec()),
            },
        )
        .unwrap()
    }

    fn message_count(db: &Db) -> u64 {
        db.conn()
            .query_row("select count(*) from messages", [], |r| r.get::<_, i64>(0))
            .unwrap() as u64
    }

    #[test]
    fn format_bytes_uses_one_scale() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(2048), "2 KB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn stats_counts_an_empty_database() {
        let (db, _) = setup();
        let dir = tempfile::tempdir().unwrap();
        let stats =
            storage_stats(&db, Path::new("/nonexistent/mailclient.sqlite"), dir.path()).unwrap();
        assert_eq!(stats.message_count, 0);
        assert_eq!(stats.db_bytes, 0);
        assert_eq!(stats.cached_files, 0);
        assert_eq!(stats.temp_files, 0);
        assert_eq!(stats.keep_per_folder, TRIM_KEEP_PER_FOLDER);
        let json = storage_stats_json(&db, Path::new("/nonexistent/x.sqlite"), dir.path()).unwrap();
        assert!(json.contains("\"message_count\":0"));
    }

    #[test]
    fn stats_counts_messages_and_cached_bytes() {
        let (db, folder) = setup();
        let mid = add_message(&db, folder, 1);
        attach(&db, mid, "a.pdf");
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1-1-a.pdf"), b"viewer copy").unwrap();
        let stats = storage_stats(&db, Path::new("/nonexistent/x.sqlite"), dir.path()).unwrap();
        assert_eq!(stats.message_count, 1);
        assert_eq!(stats.cached_files, 1);
        assert_eq!(stats.cached_bytes, 3);
        assert_eq!(stats.temp_files, 1);
        assert_eq!(stats.temp_bytes, 11);
    }

    #[test]
    fn trim_keeps_the_newest_n_per_folder() {
        let (db, folder) = setup();
        for uid in 1..=5 {
            add_message(&db, folder, uid);
        }
        let removed = trim_local_cache(&db, 3).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(message_count(&db), 3);
        let kept: Vec<u32> = db
            .conn()
            .prepare("select uid from messages order by uid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(kept, vec![3, 4, 5]);
    }

    #[test]
    fn trim_never_touches_guarded_rows() {
        let (db, folder) = setup();
        // Old rows that must all survive a keep-1 trim.
        let dirty = add_message(&db, folder, 1);
        db.conn()
            .execute("update messages set flags_dirty = 1 where id = ?1", [dirty])
            .unwrap();
        let draft = add_message(&db, folder, 2);
        db.conn()
            .execute("update messages set is_draft = 1 where id = ?1", [draft])
            .unwrap();
        let pending = add_message(&db, folder, 3);
        db.conn()
            .execute(
                "insert into pending_moves (message_id, batch, action, due_at, created_at, updated_at)
                 values (?1, 'b', 'trash', '2030-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [pending],
            )
            .unwrap();
        let queued = add_message(&db, folder, 4);
        db.conn()
            .execute(
                "insert into send_queue (account_id, message_id, status, envelope_to, created_at, updated_at)
                 values (1, ?1, 'queued', '[]', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [queued],
            )
            .unwrap();
        add_message(&db, folder, 5);
        let removed = trim_local_cache(&db, 1).unwrap();
        // Only the newest unguarded row (uid 5) is kept; uid order decides,
        // so exactly the four guarded rows plus uid 5 survive.
        assert_eq!(removed, 0);
        assert_eq!(message_count(&db), 5);
        // Without the guards the same trim would leave a single row.
        db.conn()
            .execute("update messages set flags_dirty = 0", [])
            .unwrap();
        db.conn()
            .execute("update messages set is_draft = 0", [])
            .unwrap();
        db.conn().execute("delete from pending_moves", []).unwrap();
        db.conn().execute("delete from send_queue", []).unwrap();
        let removed = trim_local_cache(&db, 1).unwrap();
        assert_eq!(removed, 4);
        assert_eq!(message_count(&db), 1);
    }

    #[test]
    fn evict_clears_bytes_but_keeps_metadata() {
        let (db, folder) = setup();
        let mid = add_message(&db, folder, 1);
        let aid = attach(&db, mid, "a.pdf");
        let evicted = evict_cached_attachments(&db).unwrap();
        assert_eq!(evicted.files, 1);
        assert_eq!(evicted.bytes_freed, 3);
        assert!(!messages::attachment_has_data(&db, aid).unwrap());
        let a = messages::get_attachment(&db, aid).unwrap();
        assert_eq!(a.filename.as_deref(), Some("a.pdf"));
        // Nothing cached: a second eviction is a no-op.
        let again = evict_cached_attachments(&db).unwrap();
        assert_eq!(again.files, 0);
    }

    #[test]
    fn evict_keeps_inline_images() {
        let (db, folder) = setup();
        let mid = add_message(&db, folder, 1);
        let file = attach(&db, mid, "a.pdf");
        let image = attach_part(&db, mid, "logo.png", true);
        let evicted = evict_cached_attachments(&db).unwrap();
        assert_eq!(evicted.files, 1);
        assert!(!messages::attachment_has_data(&db, file).unwrap());
        assert!(messages::attachment_has_data(&db, image).unwrap());
    }

    #[test]
    fn export_writes_a_snapshot_that_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("live.sqlite");
        let db = Db::open(&db_path).unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "a".into(),
                email_address: "a@example.com".into(),
                from_name: String::new(),
                imap_host: "h".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                imap_username: "u".into(),
                smtp_host: "h".into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                smtp_username: "u".into(),
                auth_vault_key: "k".into(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let f = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        add_message(&db, f, 1);
        let dest = export_database(&db, &dir.path().to_string_lossy()).unwrap();
        assert!(dest.exists());
        assert!(file_bytes(&dest) > 0);
        let reopened = Db::open(&dest).unwrap();
        let n: i64 = reopened
            .conn()
            .query_row("select count(*) from messages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn export_rejects_an_empty_destination() {
        let (db, _) = setup();
        assert!(export_database(&db, "  ").is_err());
    }

    #[test]
    fn cleanup_removes_copies_and_counts_them() {
        let base = tempfile::tempdir().unwrap();
        let temp = base.path().join("mailclient-attachments");
        std::fs::create_dir(&temp).unwrap();
        std::fs::write(temp.join("1-1-a.pdf"), b"12345").unwrap();
        std::fs::write(temp.join("2-2-b.pdf"), b"12").unwrap();
        let done = cleanup_temp_files(&temp).unwrap();
        assert_eq!(done.files_removed, 2);
        assert_eq!(done.bytes_freed, 7);
        assert!(cleanup_temp_files(&temp).unwrap().files_removed == 0);
    }
}
