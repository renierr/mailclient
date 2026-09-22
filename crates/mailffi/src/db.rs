//! Per-thread SQLite handle.
//!
//! Ported from `mailapp::bridge`, and for the same reason: opening a
//! connection costs about 2ms (file, WAL setup, migration check) to then do
//! work measured in microseconds, and this crate is entered on every star
//! toggle and every keystroke in the search box. `rusqlite::Connection` is
//! not `Sync`, so one shared handle is out; the connection is cached per
//! thread instead.
//!
//! flutter_rust_bridge answers calls on a worker pool rather than a single
//! thread, so "per thread" here means a handful of handles rather than the
//! two `mailapp` ends up with. That is fine — SQLite in WAL mode is built for
//! concurrent readers — but it is the reason the handle is leaked rather than
//! dropped: a pooled worker can outlive any single call, and a failed open
//! caches nothing, so the next call retries.

use std::cell::OnceCell;

thread_local! {
    static DB: OnceCell<&'static mailcore::Db> = const { OnceCell::new() };
}

/// The connection for this thread, opened once and reused.
pub(crate) fn shared_db() -> anyhow::Result<&'static mailcore::Db> {
    DB.with(|cell| {
        if let Some(db) = cell.get() {
            return Ok(*db);
        }
        let opened = mailcore::Db::open(&db_path())?;
        let db: &'static mailcore::Db = Box::leak(Box::new(opened));
        let _ = cell.set(db);
        Ok(db)
    })
}

/// Where the database lives for this process.
///
/// Defaults to `mailcore`'s platform path — the same file the Qt app uses, so
/// both frontends see one mailbox on a desktop where both are installed. On
/// Android there is no XDG data dir, so the host tells us the app's private
/// directory once at startup via [`set_db_dir`].
pub(crate) fn db_path() -> std::path::PathBuf {
    match db_dir_override()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
    {
        Some(dir) => dir.join("mailclient.sqlite"),
        None => mailcore::default_db_path(),
    }
}

/// Point the database at `dir` (Android: the app's private files dir).
///
/// Must be called before the first DB access; afterwards the already-opened
/// thread handles would keep pointing at the old file, so this returns an
/// error rather than silently splitting storage in two.
pub(crate) fn set_db_dir(dir: std::path::PathBuf) -> anyhow::Result<()> {
    let slot = db_dir_override();
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    if DB.with(|cell| cell.get().is_some()) {
        anyhow::bail!("database directory must be set before the first database access");
    }
    *guard = Some(dir);
    Ok(())
}

fn db_dir_override() -> &'static std::sync::Mutex<Option<std::path::PathBuf>> {
    static DIR: std::sync::OnceLock<std::sync::Mutex<Option<std::path::PathBuf>>> =
        std::sync::OnceLock::new();
    DIR.get_or_init(|| std::sync::Mutex::new(None))
}
