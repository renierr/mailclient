//! Local storage maintenance: stats, database export, temp cleanup, cache
//! trimming and attachment eviction.
//!
//! Every decision lives in `mailcore::maintenance` (shared with the Qt
//! adapter); this only adapts to the FFI surface. Payloads cross as JSON,
//! like the rest of the surface; the one count (`trim_local_cache`) crosses
//! as an integer, like `save_all_attachments_to`.

use crate::db::shared_db;

/// Storage statistics as JSON (database/message/cached/temp sizes plus the
/// per-folder keep count the trim converges to).
pub fn storage_stats_json(db_path: String, temp_dir: String) -> anyhow::Result<String> {
    let db = shared_db()?;
    Ok(mailcore::maintenance::storage_stats_json(
        db,
        std::path::Path::new(&db_path),
        std::path::Path::new(&temp_dir),
    )?)
}

/// Delete every staged viewer copy plus stale draft dirs. Returns JSON with
/// what went away (`files_removed`, `bytes_freed`, `bytes_display`,
/// `draft_dirs_removed`, `status`).
pub fn cleanup_temp_files_json(temp_dir: String) -> anyhow::Result<String> {
    let done = mailcore::maintenance::cleanup_temp_files(std::path::Path::new(&temp_dir))?;
    let status = mailcore::maintenance::cleanup_status(&done);
    Ok(serde_json::json!({
        "files_removed": done.files_removed,
        "bytes_freed": done.bytes_freed,
        "bytes_display": done.bytes_display,
        "draft_dirs_removed": done.draft_dirs_removed,
        "status": status,
    })
    .to_string())
}

/// Delete cached messages past the newest N per folder (local-only, guarded
/// rows kept). Returns how many rows went away.
pub fn trim_local_cache() -> anyhow::Result<u64> {
    Ok(mailcore::maintenance::trim_local_cache(
        shared_db()?,
        mailcore::maintenance::TRIM_KEEP_PER_FOLDER,
    )?)
}

/// Status line for [`trim_local_cache`]'s count, worded once in the core.
pub fn trim_status(removed: u64) -> String {
    mailcore::maintenance::trim_status(removed, mailcore::maintenance::TRIM_KEEP_PER_FOLDER)
}

/// Drop cached attachment bytes, keeping names and sizes. Returns JSON with
/// what went away (`files`, `bytes_freed`, `bytes_display`, `status`).
/// Cleared files download again on the next open.
pub fn evict_cached_attachments_json() -> anyhow::Result<String> {
    let evicted = mailcore::maintenance::evict_cached_attachments(shared_db()?)?;
    let status = mailcore::maintenance::evict_status(&evicted);
    Ok(serde_json::json!({
        "files": evicted.files,
        "bytes_freed": evicted.bytes_freed,
        "bytes_display": evicted.bytes_display,
        "status": status,
    })
    .to_string())
}

/// Write a consistent snapshot of the database to `path` (a directory gets
/// `mailclient-backup.sqlite`). Returns where it went.
pub fn export_database_to(path: String) -> anyhow::Result<String> {
    let dest = mailcore::maintenance::export_database(shared_db()?, &path)?;
    Ok(dest.to_string_lossy().into_owned())
}
