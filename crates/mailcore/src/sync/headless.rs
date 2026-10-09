//! Headless sync: the shared "sync one/all accounts" orchestration used by
//! both the GUI (`mailapp` bridge) and the `--sync-once` CLI behind the
//! Omarchy bar widget.
//!
//! The GUI keeps its pooled IMAP sessions and only borrows
//! [`sync_account`]; the CLI builds a fresh [`ImapSync`] per account inside
//! [`sync_all_accounts`]. Either way the folder sweep, windows, and counts
//! cannot drift apart.
//!
//! [`SyncLock`] serializes the `--sync-once` runs (CLI + bar timer) with each
//! other: SQLite WAL already prevents corruption, the lock turns collisions
//! into a clean "skip this run" instead of a `database is locked` error. The
//! GUI does not take it — a user-driven sync or send must never be skipped —
//! so a GUI and a CLI run can overlap. The outbox is safe under that overlap
//! because every submit first wins an atomic [`crate::store::queue::claim`].

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::auth;
use crate::db::Db;
use crate::error::Result;
use crate::models::{Account, Folder, FolderRole};
use crate::store::{accounts, folders, messages, pending_moves, settings};
use crate::sync::imap::{full_discovery_due, ImapSync, FULL_SYNC_WINDOW, QUICK_SYNC_WINDOW};
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

impl AccountSyncResult {
    /// Whether the inbox itself synced. That is what a scheduled check is
    /// for; an outbox or other-folder failure alongside it must not make
    /// the account count as unchecked, or it would be retried on every
    /// tick regardless of its interval.
    #[must_use]
    pub fn inbox_checked(&self) -> bool {
        self.folders
            .iter()
            .any(|f| f.role == FolderRole::Inbox.as_str())
    }
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

/// Which folders [`sync_account`] covers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SyncScope {
    /// Every subscribed folder (INBOX full window, the rest quick).
    #[default]
    All,
    /// Inbox-role folders only, over the cached folder tree when it already
    /// has an inbox (no LIST round-trips). For the Android background tick,
    /// which must finish inside a short Doze maintenance window.
    InboxOnly,
}

impl SyncScope {
    /// Whether [`sync_account`] syncs `folder` under this scope (the
    /// subscription check comes on top).
    #[must_use]
    pub fn covers(self, folder: &Folder) -> bool {
        self == SyncScope::All || folder.role == FolderRole::Inbox
    }

    /// Whether the cached folder tree is enough to sync this scope without
    /// a LIST: only for inbox-only syncs, and only once a subscribed inbox
    /// is cached — a fresh account still discovers its folders first.
    #[must_use]
    pub fn cached_tree_suffices(self, cached: &[Folder]) -> bool {
        self == SyncScope::InboxOnly
            && cached
                .iter()
                .any(|f| f.role == FolderRole::Inbox && f.subscribed)
    }
}

/// Per-step sync progress: ready status strings for the GUI status line.
/// String-based (not counts) so phases without totals — connecting, the
/// folder list — report too, instead of staying silent. Use
/// [`sync_progress_status`] / [`sync_folder_progress_status`] to format the
/// counted steps; the headless CLI passes `None`.
pub type SyncProgress<'a> = &'a dyn Fn(String);

/// The status line every frontend shows for one [`SyncProgress`] folder
/// milestone.
pub fn sync_progress_status(done: usize, total: usize, path: &str) -> String {
    format!("Syncing {done}/{total}: {path}")
}

/// The status line for within-folder fetch progress, reported per chunk so
/// a big first window is visibly moving instead of one long silence.
pub fn sync_folder_progress_status(done: usize, total: usize, path: &str) -> String {
    format!("Syncing {path} ({done}/{total})")
}

/// Sync one account over an already-connected session.
///
/// Flushes the SMTP outbox, pushes local flag changes, refreshes the folder
/// list, then syncs every subscribed folder (INBOX full window, the rest
/// quick) — or only the inbox, per [`SyncScope`]. Per-folder failures are
/// recorded in `errors` and skipped.
pub async fn sync_account(
    db: &Db,
    account: &Account,
    imap: &mut ImapSync,
    scope: SyncScope,
    progress: Option<SyncProgress<'_>>,
) -> AccountSyncResult {
    let mut out = AccountSyncResult {
        account_id: account.id,
        email: account.email_address.clone(),
        ..Default::default()
    };

    let secrets = match auth::load_account_secrets_retry(&account.auth_vault_key).await {
        Ok(s) => s,
        Err(e) => {
            // `StoreError::Keyring` already displays with a "keyring: " prefix.
            out.errors.push(format!("{e}"));
            out.unread = unread_for_account(db, account.id);
            return out;
        }
    };

    let sender = SmtpSender::new(account);
    match sender
        .flush_outbox(
            db,
            account.id,
            &secrets.smtp_password,
            Some(secrets.imap_password.as_str()),
        )
        .await
    {
        Ok(n) if n > 0 => log::info!("smtp: flushed {n} queued send(s)"),
        Err(e) => out.errors.push(format!("outbox: {e}")),
        _ => {}
    }

    out.pushed_flags += imap.push_dirty_flags(db, account.id).await;
    // After the flags: a queued move changes the message's UID.
    imap.push_due_moves(db, account.id).await;

    // Throttled discovery: one LIST pass every run, full four-pass
    // discovery only when the tree changed or the interval lapsed —
    // passes 2-4 cost ~30 round-trips and dominated every Gmail start.
    let cached = folders::list_by_account(db, account.id).unwrap_or_default();
    let cached_inbox = scope.cached_tree_suffices(&cached);
    let cached_paths: std::collections::HashSet<String> =
        cached.iter().map(|f| f.path.clone()).collect();
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let remote = if cached_inbox {
        cached
    } else {
        if let Some(report) = progress {
            report("Updating folder list…".to_string());
        }
        match imap.sync_folders_quick(db, account.id).await {
            Ok(quick) => {
                let quick_paths: std::collections::HashSet<String> =
                    quick.iter().map(|f| f.path.clone()).collect();
                let last_full = settings::get_last_full_discovery(db, account.id);
                if full_discovery_due(&cached_paths, &quick_paths, last_full, now_unix) {
                    match imap.sync_folders(db, account.id).await {
                        Ok(full) => full,
                        // Quick list already upserted: still syncable, note it.
                        Err(e) => {
                            out.errors.push(format!("folder list (full): {e}"));
                            quick
                        }
                    }
                } else {
                    log::debug!("imap: folder tree unchanged and fresh, keeping quick LIST");
                    quick
                }
            }
            Err(e) => {
                out.errors.push(format!("folder list: {e}"));
                out.unread = unread_for_account(db, account.id);
                return out;
            }
        }
    };

    let subscribed_total = remote
        .iter()
        .filter(|f| f.subscribed && scope.covers(f))
        .count();
    let mut folders_attempted = 0usize;
    for f in &remote {
        if !f.subscribed {
            out.folders_skipped_hidden += 1;
            continue;
        }
        if !scope.covers(f) {
            continue;
        }
        folders_attempted += 1;
        if let Some(report) = progress {
            report(sync_progress_status(
                folders_attempted,
                subscribed_total,
                &f.path,
            ));
        }
        let window = if f.role == FolderRole::Inbox {
            FULL_SYNC_WINDOW
        } else {
            QUICK_SYNC_WINDOW
        };
        // Chunk-level progress inside the folder (same status channel).
        let folder_progress = |status: String| {
            if let Some(report) = progress {
                report(status);
            }
        };
        match imap
            .sync_folder_window(db, f.id, Some(window), Some(&folder_progress))
            .await
        {
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
            Err(e) => {
                out.errors.push(format!("{}: {e}", f.path));
                // A dead connection fails every remaining folder the same
                // way; stop so the next run reconnects instead (B3).
                if !imap.is_connected() {
                    out.errors
                        .push("connection lost, remaining folders skipped".to_string());
                    break;
                }
            }
        }
    }

    out.unread = out
        .folders
        .iter()
        .filter(|f| f.role == FolderRole::Inbox.as_str())
        .map(|f| f.unread)
        .sum::<u64>();
    if out.folders.is_empty() {
        // Folder list failed silently or nothing subscribed: fall back to cache.
        out.unread = unread_for_account(db, account.id);
    }
    out
}

/// Sync every account with a fresh connection each. One account failing
/// (bad password, offline) never stops the rest.
pub async fn sync_all_accounts(db: &Db, scope: SyncScope) -> SyncAllReport {
    match accounts::list(db) {
        Ok(list) => sync_accounts(db, &list, scope).await,
        Err(e) => SyncAllReport {
            errors: vec![format!("accounts: {e}")],
            ..Default::default()
        },
    }
}

/// [`sync_all_accounts`] over just `list`, one fresh connection each.
pub async fn sync_accounts(db: &Db, list: &[Account], scope: SyncScope) -> SyncAllReport {
    let mut report = SyncAllReport::default();
    for acc in list {
        let mut imap = ImapSync::new(acc);
        let secrets = auth::load_account_secrets_retry(&acc.auth_vault_key).await;
        let result = match secrets {
            Ok(s) => match imap.connect(&s.imap_password).await {
                Ok(()) => {
                    let r = sync_account(db, acc, &mut imap, scope, None).await;
                    imap.logout().await;
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
                r.errors.push(format!("{e}"));
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

/// Push one account's queued local changes — read/star flags, then moves
/// that are due — over a fresh connection, for callers without the app's
/// pooled sessions, such as a notification's "Mark read" or "Archive" while
/// the app is closed. Returns how many went out; whatever fails stays
/// queued (`flags_dirty`, `pending_moves`) for the next sync.
pub async fn push_changes(db: &Db, account_id: i64) -> Result<u64> {
    let flags_dirty = !messages::list_flags_dirty(db, account_id)?.is_empty();
    let moves_due = !pending_moves::list_due(db, account_id, &crate::store::now())?.is_empty();
    if !flags_dirty && !moves_due {
        return Ok(0);
    }
    let acc = accounts::get(db, account_id)?;
    let secrets = auth::load_account_secrets_retry(&acc.auth_vault_key).await?;
    let mut imap = ImapSync::new(&acc);
    imap.connect(&secrets.imap_password).await?;
    // Flags first: a toggle on a message about to move must reach the
    // server while its UID is still valid in the source folder.
    let pushed = imap.push_dirty_flags(db, acc.id).await + imap.push_due_moves(db, acc.id).await;
    imap.logout().await;
    Ok(pushed)
}

/// Synchronous wrapper around [`push_changes`] for FFI callers.
pub fn push_changes_blocking(db: &Db, account_id: i64) -> Result<u64> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime for change push");
    rt.block_on(push_changes(db, account_id))
}

/// Synchronous wrapper around [`sync_all_accounts`] for CLI callers.
pub fn sync_all_accounts_blocking(db: &Db) -> SyncAllReport {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime for headless sync");
    rt.block_on(sync_all_accounts(db, SyncScope::All))
}

/// Cached unread total for one account (no network).
///
/// Inbox-only: only folders with the Inbox role count as new mail.
/// Trash/Sent/Drafts/Archive/Junk unread never affects the badge,
/// popup, or notify-on-rise.
#[must_use]
pub fn unread_for_account(db: &Db, account_id: i64) -> u64 {
    folders::list_by_account(db, account_id)
        .unwrap_or_default()
        .iter()
        .filter(|f| f.role == FolderRole::Inbox)
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
///
/// Inbox-only: only Inbox-role folders are listed, so the popup agrees
/// with the inbox-only badge.
#[must_use]
pub fn recent_unread(db: &Db, limit: u64, account_id: Option<i64>) -> Vec<RecentUnread> {
    let sql = "select m.account_id, a.email_address, f.path,
                      coalesce(m.from_addr, ''), coalesce(m.subject, ''), coalesce(m.date, '')
               from messages m
               join accounts a on a.id = m.account_id
               join folders f on f.id = m.folder_id
               where m.is_read = 0
                 and f.role = 'inbox'
                 and m.id not in (select message_id from pending_moves)
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

/// Cross-process sync mutex: an OS file lock (`flock` on Linux and
/// Android, `LockFileEx` on Windows) on `<db-dir>/.sync.lock`. A live
/// holder makes [`acquire_sync_lock`] return `None` so the loser skips its
/// run quietly; the kernel drops the lock when the holder exits or dies.
///
/// It used to be a create-new file holding the holder's pid, reaped when
/// the pid looked dead or the file was 15 minutes old. Two runs could both
/// judge it stale (an empty file mid-write parsed as "stale") and both
/// take it, and the reaping retried by unbounded recursion (B18).
///
/// `_file` is `None` on a filesystem that cannot lock at all (see
/// [`acquire_sync_lock`]): the guard then excludes nobody.
pub struct SyncLock {
    _file: Option<std::fs::File>,
}

impl SyncLock {
    fn lock_path(db_path: &Path) -> PathBuf {
        db_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .join(".sync.lock")
    }
}

/// Try to take the sync lock for `db_path`. `Ok(None)` = loss to a live
/// holder (skip the run); `Ok(Some(guard))` = we hold it until the guard
/// drops; `Err` = IO failure.
///
/// The file itself is never deleted: a run that opened it before the
/// delete would lock the unlinked file while the next one creates and
/// locks a new one, and both would hold "the" lock.
///
/// A filesystem that cannot lock at all — proven on an Android emulator's
/// data dir, where `try_lock` answers "not supported" — gets an unlocked
/// guard instead of an error. Refusing every check there would cost the
/// user far more than losing the exclusion ever could: the lock only
/// matters when two runs of one database really overlap, and a skipped
/// run is indistinguishable from a broken feature.
pub fn acquire_sync_lock(db_path: &Path) -> Result<Option<SyncLock>> {
    let path = SyncLock::lock_path(db_path);
    let io_error = |e: std::io::Error| {
        crate::error::StoreError::InvalidInput(format!("sync lock {}: {e}", path.display()))
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(io_error)?;
    match file.try_lock() {
        Ok(()) => {
            // The pid is only a hint for a human looking at the file.
            if file.set_len(0).is_ok() {
                let _ = writeln!(&file, "{}", std::process::id());
            }
            Ok(Some(SyncLock { _file: Some(file) }))
        }
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) if lock_unsupported(&e) => {
            log::warn!(
                "sync lock cannot be taken on {} ({e}); syncing without the cross-process lock",
                path.display()
            );
            Ok(Some(SyncLock { _file: None }))
        }
        Err(std::fs::TryLockError::Error(e)) => Err(io_error(e)),
    }
}

/// Whether a failed `try_lock` means "this filesystem cannot lock" rather
/// than a real problem: the documented [`std::io::ErrorKind::Unsupported`],
/// or the wording a platform uses for the same thing.
fn lock_unsupported(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::Unsupported || e.to_string().contains("not supported")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount};
    use crate::store::{accounts, folders, messages};

    #[test]
    fn the_sync_lock_is_held_until_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("db.sqlite");
        let held = acquire_sync_lock(&db_path).unwrap().expect("free lock");
        assert!(acquire_sync_lock(&db_path).unwrap().is_none(), "held");
        drop(held);
        assert!(acquire_sync_lock(&db_path).unwrap().is_some(), "released");
    }

    #[test]
    fn a_leftover_lock_file_does_not_block() {
        // An empty file (a holder mid-write) used to read as stale for one
        // run and live for another; a file is no lock now, only the OS lock.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("db.sqlite");
        for content in ["", "not a pid\n", "4294967295\n"] {
            std::fs::write(SyncLock::lock_path(&db_path), content).unwrap();
            assert!(
                acquire_sync_lock(&db_path).unwrap().is_some(),
                "{content:?}"
            );
        }
    }

    #[test]
    fn a_filesystem_that_cannot_lock_still_gets_its_check() {
        // What the Android emulator's data dir answers: try_lock() fails
        // with "not supported", which must not skip every check.
        let unsupported =
            std::io::Error::new(std::io::ErrorKind::Unsupported, "try_lock() not supported");
        assert!(lock_unsupported(&unsupported));
        assert!(lock_unsupported(&std::io::Error::other(
            "try_lock() not supported"
        )));
        // A real IO problem is still an error, not a degraded guard.
        assert!(!lock_unsupported(&std::io::Error::other("no space left")));
        assert!(!lock_unsupported(&std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "denied"
        )));
        // Losing the race stays a loss: no guard, run skipped.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("db.sqlite");
        let held = acquire_sync_lock(&db_path).unwrap().expect("free lock");
        assert!(acquire_sync_lock(&db_path).unwrap().is_none());
        drop(held);
    }

    #[test]
    fn progress_status_names_step_total_and_folder() {
        assert_eq!(sync_progress_status(3, 9, "Sent"), "Syncing 3/9: Sent");
    }

    #[test]
    fn an_outbox_error_still_counts_the_inbox_as_checked() {
        let inbox = FolderSyncSummary {
            role: FolderRole::Inbox.as_str().to_string(),
            ..Default::default()
        };
        let mut r = AccountSyncResult {
            errors: vec!["outbox: smtp down".into()],
            ..Default::default()
        };
        assert!(!r.inbox_checked(), "no inbox synced yet");
        r.folders.push(inbox);
        assert!(r.inbox_checked());
    }

    pub(crate) fn setup_db() -> (Db, i64, i64, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "a".to_string(),
                email_address: "a@example.com".to_string(),
                from_name: String::new(),
                imap_host: "h".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "h".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let inbox = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        let trash = folders::upsert(&db, acc, "Trash", "/", FolderRole::Trash).unwrap();
        (db, acc, inbox, trash)
    }

    #[test]
    fn inbox_only_scope_covers_the_inbox_and_reuses_a_cached_tree() {
        let (db, acc, _, _) = setup_db();
        let cached = folders::list_by_account(&db, acc).unwrap();
        let (inbox, trash): (Vec<_>, Vec<_>) =
            cached.iter().partition(|f| f.role == FolderRole::Inbox);
        assert!(SyncScope::InboxOnly.covers(inbox[0]));
        assert!(!SyncScope::InboxOnly.covers(trash[0]));
        assert!(SyncScope::All.covers(trash[0]));

        assert!(SyncScope::InboxOnly.cached_tree_suffices(&cached));
        assert!(!SyncScope::All.cached_tree_suffices(&cached));
        // No cached inbox yet (fresh account): discover over LIST first.
        assert!(!SyncScope::InboxOnly.cached_tree_suffices(&[]));
        let unsubscribed: Vec<Folder> = cached
            .into_iter()
            .map(|mut f| {
                f.subscribed = false;
                f
            })
            .collect();
        assert!(!SyncScope::InboxOnly.cached_tree_suffices(&unsubscribed));
    }

    #[test]
    fn unread_counts_inbox_only() {
        let (db, acc, inbox, trash) = setup_db();
        let mut m1 = messages::sample_new(acc, inbox, 1);
        m1.is_read = false;
        messages::upsert(&db, &m1).unwrap();
        let mut m2 = messages::sample_new(acc, trash, 2);
        m2.is_read = false;
        messages::upsert(&db, &m2).unwrap();
        // Trash unread must not affect the badge.
        assert_eq!(unread_for_account(&db, acc), 1);
        assert_eq!(unread_summary(&db)[0].unread, 1);
    }

    #[test]
    fn recent_unread_lists_inbox_only() {
        let (db, acc, inbox, trash) = setup_db();
        let mut m1 = messages::sample_new(acc, inbox, 1);
        m1.is_read = false;
        m1.subject = Some("inbox mail".to_string());
        messages::upsert(&db, &m1).unwrap();
        let mut m2 = messages::sample_new(acc, trash, 2);
        m2.is_read = false;
        m2.subject = Some("trash mail".to_string());
        messages::upsert(&db, &m2).unwrap();
        let recent = recent_unread(&db, 10, None);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].subject, "inbox mail");
    }
}
