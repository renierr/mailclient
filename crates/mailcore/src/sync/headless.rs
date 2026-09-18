//! Headless sync: the shared "sync one/all accounts" orchestration used by
//! both the GUI (`mailapp` bridge) and the `--sync-once` CLI behind the
//! Omarchy bar widget.
//!
//! The GUI keeps its pooled IMAP sessions and only borrows
//! [`sync_account`]; the CLI builds a fresh [`ImapSync`] per account inside
//! [`sync_all_accounts`]. Either way the folder sweep, windows, and counts
//! cannot drift apart.
//!
//! [`SyncLock`] serializes writers across processes (GUI + CLI + timer):
//! SQLite WAL already prevents corruption, the lock turns collisions into a
//! clean "skip this run" instead of a `database is locked` error.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::auth;
use crate::db::Db;
use crate::error::Result;
use crate::models::{Account, FolderRole};
use crate::store::{accounts, folders, messages};
use crate::sync::imap::{ImapSync, FULL_SYNC_WINDOW, QUICK_SYNC_WINDOW};
use crate::sync::sender::SmtpSender;
use crate::sync::traits::SyncProvider;

/// Per-folder outcome inside [`AccountSyncResult`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct FolderSyncSummary {
    pub folder_id: i64,
    pub path: String,
    pub role: String,
    pub fetched: u64,
    pub expunged: u64,
    pub unread: u64,
}

/// Outcome of syncing one account. Errors are collected, never fatal:
/// one broken folder must not hide the rest.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AccountSyncResult {
    pub account_id: i64,
    pub email: String,
    pub folders_synced: usize,
    pub folders_skipped_hidden: usize,
    pub fetched: u64,
    pub expunged: u64,
    pub pushed_flags: u64,
    pub unread: u64,
    pub folders: Vec<FolderSyncSummary>,
    pub errors: Vec<String>,
}

/// Outcome of [`sync_all_accounts`].
#[derive(Debug, Clone, Default, Serialize)]
pub struct SyncAllReport {
    pub accounts: Vec<AccountSyncResult>,
    pub total_fetched: u64,
    pub total_expunged: u64,
    pub total_unread: u64,
    pub errors: Vec<String>,
}

/// One unread message for notification/popup display. Metadata only —
/// never bodies.
#[derive(Debug, Clone, Default, Serialize)]
pub struct RecentUnread {
    pub account_id: i64,
    pub account_email: String,
    pub folder: String,
    pub from: String,
    pub subject: String,
    pub date: String,
}

/// Sync one account over an already-connected session.
///
/// Flushes the SMTP outbox, pushes local flag changes, refreshes the folder
/// list, then syncs every subscribed folder (INBOX full window, the rest
/// quick). Per-folder failures are recorded in `errors` and skipped.
pub fn sync_account(db: &Db, account: &Account, imap: &mut ImapSync) -> AccountSyncResult {
    let mut out = AccountSyncResult {
        account_id: account.id,
        email: account.email_address.clone(),
        ..Default::default()
    };

    let secrets = match auth::load_account_secrets(&account.auth_vault_key) {
        Ok(s) => s,
        Err(e) => {
            out.errors.push(format!("keyring: {e}"));
            out.unread = unread_for_account(db, account.id);
            return out;
        }
    };

    let sender = SmtpSender::new(account);
    match sender.flush_outbox(
        db,
        account.id,
        &secrets.smtp_password,
        Some(secrets.imap_password.as_str()),
    ) {
        Ok(n) if n > 0 => log::info!("smtp: flushed {n} queued send(s)"),
        Err(e) => out.errors.push(format!("outbox: {e}")),
        _ => {}
    }

    for m in messages::list_flags_dirty(db, account.id).unwrap_or_default() {
        if imap.push_flags(db, &m).is_ok() {
            let _ = messages::clear_flags_dirty(db, m.id);
            out.pushed_flags += 1;
        }
    }

    let remote = match imap.sync_folders(db, account.id) {
        Ok(f) => f,
        Err(e) => {
            out.errors.push(format!("folder list: {e}"));
            out.unread = unread_for_account(db, account.id);
            return out;
        }
    };

    for f in &remote {
        if !f.subscribed {
            out.folders_skipped_hidden += 1;
            continue;
        }
        let window = if f.role == FolderRole::Inbox {
            FULL_SYNC_WINDOW
        } else {
            QUICK_SYNC_WINDOW
        };
        match imap.sync_folder_window(db, f.id, Some(window)) {
            Ok(r) => {
                out.fetched += r.fetched;
                out.expunged += r.expunged;
                out.folders_synced += 1;
                out.folders.push(FolderSyncSummary {
                    folder_id: f.id,
                    path: f.path.clone(),
                    role: format!("{:?}", f.role).to_lowercase(),
                    fetched: r.fetched,
                    expunged: r.expunged,
                    unread: messages::count_unread(db, f.id).unwrap_or(0),
                });
            }
            Err(e) => out.errors.push(format!("{}: {e}", f.path)),
        }
    }

    out.unread = out.folders.iter().map(|f| f.unread).sum::<u64>();
    if out.folders.is_empty() {
        // Folder list failed silently or nothing subscribed: fall back to cache.
        out.unread = unread_for_account(db, account.id);
    }
    out
}

/// Sync every account with a fresh connection each. One account failing
/// (bad password, offline) never stops the rest.
pub fn sync_all_accounts(db: &Db) -> SyncAllReport {
    let mut report = SyncAllReport::default();
    let list = match accounts::list(db) {
        Ok(a) => a,
        Err(e) => {
            report.errors.push(format!("accounts: {e}"));
            return report;
        }
    };
    for acc in &list {
        let mut imap = ImapSync::new(acc);
        let secrets = auth::load_account_secrets(&acc.auth_vault_key);
        let result = match secrets {
            Ok(s) => match imap.connect(&s.imap_password) {
                Ok(()) => {
                    let r = sync_account(db, acc, &mut imap);
                    imap.disconnect();
                    r
                }
                Err(e) => {
                    let mut r = AccountSyncResult {
                        account_id: acc.id,
                        email: acc.email_address.clone(),
                        ..Default::default()
                    };
                    r.errors.push(format!("connect: {e}"));
                    r.unread = unread_for_account(db, acc.id);
                    r
                }
            },
            Err(e) => {
                let mut r = AccountSyncResult {
                    account_id: acc.id,
                    email: acc.email_address.clone(),
                    ..Default::default()
                };
                r.errors.push(format!("keyring: {e}"));
                r.unread = unread_for_account(db, acc.id);
                r
            }
        };
        report.total_fetched += result.fetched;
        report.total_expunged += result.expunged;
        report.total_unread += result.unread;
        report.errors.extend(
            result
                .errors
                .iter()
                .map(|e| format!("{}: {e}", acc.email_address)),
        );
        report.accounts.push(result);
    }
    report
}

/// Cached unread total for one account (no network).
#[must_use]
pub fn unread_for_account(db: &Db, account_id: i64) -> u64 {
    folders::list_by_account(db, account_id)
        .unwrap_or_default()
        .iter()
        .map(|f| messages::count_unread(db, f.id).unwrap_or(0))
        .sum()
}

/// Cached unread totals for all accounts (no network).
pub fn unread_summary(db: &Db) -> Vec<AccountSyncResult> {
    accounts::list(db)
        .unwrap_or_default()
        .iter()
        .map(|a| AccountSyncResult {
            account_id: a.id,
            email: a.email_address.clone(),
            unread: unread_for_account(db, a.id),
            ..Default::default()
        })
        .collect()
}

/// Newest unread messages (metadata only, no network). Ordered
/// newest-first, capped at `limit`. When `account_id` is set, only that
/// account is listed so the popup agrees with a filtered unread count.
#[must_use]
pub fn recent_unread(db: &Db, limit: u64, account_id: Option<i64>) -> Vec<RecentUnread> {
    let sql = "select m.account_id, a.email_address, f.path,
                      coalesce(m.from_addr, ''), coalesce(m.subject, ''), coalesce(m.date, '')
               from messages m
               join accounts a on a.id = m.account_id
               join folders f on f.id = m.folder_id
               where m.is_read = 0
                 and (?2 is null or m.account_id = ?2)
               order by m.date desc, m.id desc
               limit ?1";
    let rows = db.conn().prepare(sql).and_then(|mut stmt| {
        stmt.query_map((limit as i64, account_id), |row| {
            Ok(RecentUnread {
                account_id: row.get(0)?,
                account_email: row.get(1)?,
                folder: row.get(2)?,
                from: row.get(3)?,
                subject: row.get(4)?,
                date: row.get(5)?,
            })
        })
        .and_then(|mapped| mapped.collect::<rusqlite::Result<Vec<_>>>())
    });
    rows.unwrap_or_default()
}

/// Cross-process sync mutex. The lock file lives next to the DB
/// (`<db-dir>/.sync.lock`) and holds the holder's pid. Stale locks
/// (dead pid, or older than 15 minutes) are reaped; a live holder makes
/// [`acquire_sync_lock`] fail so the loser skips its run quietly.
pub struct SyncLock {
    path: PathBuf,
    done: bool,
}

impl SyncLock {
    fn lock_path(db_path: &Path) -> PathBuf {
        db_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join(".sync.lock")
    }

    fn pid_alive(pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        {
            // A live process has a /proc entry; a zombie/reaped pid does not.
            // PID reuse is harmless here: at worst we skip one background run.
            Path::new(&format!("/proc/{pid}")).exists()
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = pid;
            true
        }
    }
}

/// Try to take the sync lock for `db_path`. `Ok(None)` = loss to a live
/// holder (skip the run); `Ok(Some(guard))` = we hold it; `Err` = IO failure.
pub fn acquire_sync_lock(db_path: &Path) -> Result<Option<SyncLock>> {
    let path = SyncLock::lock_path(db_path);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut f) => {
            let _ = writeln!(f, "{}", std::process::id());
            Ok(Some(SyncLock { path, done: false }))
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let stale = std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .is_none_or(|pid| {
                    !SyncLock::pid_alive(pid) || lock_age(&path).unwrap_or_default() > 900
                });
            if stale {
                let _ = std::fs::remove_file(&path);
                return acquire_sync_lock(db_path);
            }
            Ok(None)
        }
        Err(e) => Err(crate::error::StoreError::InvalidInput(format!(
            "sync lock {}: {e}",
            path.display()
        ))),
    }
}

fn lock_age(path: &Path) -> std::io::Result<u64> {
    let mtime = std::fs::metadata(path)?.modified()?;
    Ok(std::time::SystemTime::now()
        .duration_since(mtime)
        .map(|d| d.as_secs())
        .unwrap_or(u64::MAX))
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        if !self.done {
            self.done = true;
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
