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

use serde::{Deserialize, Serialize};

use crate::auth;
use crate::db::Db;
use crate::error::Result;
use crate::models::{Account, FolderRole};
use crate::store::{accounts, folders, messages, settings};
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

/// One mail the background check has never reported before. Carries the ids
/// the UI needs to open it (`account_id`/`folder_id`/`uid`), plus the
/// metadata a notification shows. Bodies never cross here.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NewMail {
    pub account_id: i64,
    pub account_email: String,
    pub folder_id: i64,
    pub folder: String,
    pub uid: u32,
    /// Folder UIDVALIDITY this UID belongs to. A server reset restarts UIDs
    /// from low numbers under a new validity, so a notified-UID mark that
    /// ignores it would silence every later arrival.
    pub uid_validity: u32,
    pub from: String,
    pub subject: String,
    pub date: String,
}

/// Outcome of [`background_check`]: what arrived since the previous check.
#[derive(Debug, Clone, Default, Serialize)]
pub struct BackgroundReport {
    /// Another sync held the lock, so this run did nothing. Not an error —
    /// the next scheduled run picks it up.
    pub skipped: bool,
    /// Unread mail first seen by this check. Empty on the very first run:
    /// that run only records the baseline so pre-existing unread mail does
    /// not notify all at once.
    pub new: Vec<NewMail>,
    /// Marks to hand to [`commit_seen`] once `new` has been notified (or
    /// deliberately not, with alerts off). Uncommitted, `new` is reported
    /// again on the next check.
    pub marks: Vec<SeenMark>,
    /// Cached inbox-unread total across accounts, for the launcher badge.
    pub total_unread: u64,
    pub errors: Vec<String>,
}

/// Prefix for background-check high-water marks (internal bookkeeping, not
/// a user preference, so deliberately outside the settings allowlist):
/// `bg_seen_uid_{account_id}_{folder_id}` stores `{uidvalidity}:{max_uid}`.
pub const BG_SEEN_PREFIX: &str = "bg_seen_uid_";

/// Per-folder sync progress, called as `report(done, total, path)` before
/// each folder sync so the GUI can show "3/15: …" instead of a bare
/// spinner; the headless CLI passes `None`.
pub type SyncProgress<'a> = &'a dyn Fn(usize, usize, &str);

/// Per-folder high-water mark from [`collect_new_mail`], stored by
/// [`commit_seen`]. Crosses to Dart and back unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenMark {
    pub account_id: i64,
    pub folder_id: i64,
    pub uid_validity: i64,
    pub uid: i64,
}

impl SeenMark {
    fn key(&self) -> String {
        format!("{BG_SEEN_PREFIX}{}_{}", self.account_id, self.folder_id)
    }
}

/// Sync one account over an already-connected session.
///
/// Flushes the SMTP outbox, pushes local flag changes, refreshes the folder
/// list, then syncs every subscribed folder (INBOX full window, the rest
/// quick). Per-folder failures are recorded in `errors` and skipped.
pub async fn sync_account(
    db: &Db,
    account: &Account,
    imap: &mut ImapSync,
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

    // Throttled discovery: one LIST pass every run, full four-pass
    // discovery only when the tree changed or the interval lapsed —
    // passes 2-4 cost ~30 round-trips and dominated every Gmail start.
    let cached_paths: std::collections::HashSet<String> = folders::list_by_account(db, account.id)
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.path)
        .collect();
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let remote = match imap.sync_folders_quick(db, account.id).await {
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
    };

    let subscribed_total = remote.iter().filter(|f| f.subscribed).count();
    let mut folders_attempted = 0usize;
    for f in &remote {
        if !f.subscribed {
            out.folders_skipped_hidden += 1;
            continue;
        }
        folders_attempted += 1;
        if let Some(report) = progress {
            report(folders_attempted, subscribed_total, &f.path);
        }
        let window = if f.role == FolderRole::Inbox {
            FULL_SYNC_WINDOW
        } else {
            QUICK_SYNC_WINDOW
        };
        match imap.sync_folder_window(db, f.id, Some(window)).await {
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
pub async fn sync_all_accounts(db: &Db) -> SyncAllReport {
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
        let secrets = auth::load_account_secrets_retry(&acc.auth_vault_key).await;
        let result = match secrets {
            Ok(s) => match imap.connect(&s.imap_password).await {
                Ok(()) => {
                    let r = sync_account(db, acc, &mut imap, None).await;
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

/// Synchronous wrapper around [`sync_all_accounts`] for CLI callers.
pub fn sync_all_accounts_blocking(db: &Db) -> SyncAllReport {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime for headless sync");
    rt.block_on(sync_all_accounts(db))
}

/// One background tick: lock, sync every account over fresh connections,
/// and report mail that arrived since the previous tick.
///
/// The lock turns overlap with a foreground sync (or a second worker) into
/// a quiet skip rather than a `database is locked` error. Fresh connections
/// keep this off the GUI's pooled sessions, so a worker run can never steal
/// or stall the session the user is reading through.
pub async fn background_check(db: &Db, db_path: &Path) -> BackgroundReport {
    let _lock = match acquire_sync_lock(db_path) {
        Ok(Some(guard)) => guard,
        Ok(None) => {
            return BackgroundReport {
                skipped: true,
                total_unread: cached_total_unread(db),
                ..Default::default()
            };
        }
        Err(e) => {
            return BackgroundReport {
                skipped: true,
                total_unread: cached_total_unread(db),
                errors: vec![format!("sync lock: {e}")],
                ..Default::default()
            };
        }
    };
    // NB: `skipped` stays false here — losing the lock returns above.
    let report = sync_all_accounts(db).await;
    let (new, marks) = collect_new_mail(db);
    BackgroundReport {
        skipped: false,
        new,
        marks,
        total_unread: report.total_unread,
        errors: report.errors,
    }
}

/// Synchronous wrapper around [`background_check`] for FFI callers, which
/// cannot await. Same current-thread runtime shape as
/// [`sync_all_accounts_blocking`].
pub fn background_check_blocking(db: &Db, db_path: &Path) -> BackgroundReport {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime for background check");
    rt.block_on(background_check(db, db_path))
}

/// Unread inbox mail first seen since the marks were last committed.
///
/// Each inbox folder keeps a `{uidvalidity}:{max_uid}` mark under
/// [`BG_SEEN_PREFIX`]. A missing mark or a changed validity means "never
/// reliably seen": the folder's current top becomes the baseline and nothing
/// is reported, so enabling the feature (or a server-side UID reset) does not
/// ding for every old message at once. Otherwise unread mail above the mark
/// is new.
///
/// Read-only: the returned marks move the high-water line only once passed
/// to [`commit_seen`], which the caller does after the notification was
/// posted — a failed post then reports the same mail again next time. The
/// top is taken over every inbox message, read or not, and an empty inbox
/// baselines at 0, so the first mail into a fully read inbox still counts.
pub fn collect_new_mail(db: &Db) -> (Vec<NewMail>, Vec<SeenMark>) {
    let tops: Vec<SeenMark> = db
        .conn()
        .prepare(
            "select f.account_id, f.id, coalesce(f.uid_validity, 0),
                        coalesce(max(m.uid), 0)
                 from folders f
                 left join messages m on m.folder_id = f.id
                 where f.role = 'inbox'
                 group by f.account_id, f.id",
        )
        .and_then(|mut stmt| {
            stmt.query_map([], |row| {
                Ok(SeenMark {
                    account_id: row.get(0)?,
                    folder_id: row.get(1)?,
                    uid_validity: row.get(2)?,
                    uid: row.get(3)?,
                })
            })
            .and_then(|mapped| mapped.collect::<rusqlite::Result<Vec<_>>>())
        })
        .unwrap_or_default();

    let mut out = Vec::new();
    for top in &tops {
        let seen: Option<(i64, i64)> = settings::get(db, &top.key()).ok().flatten().and_then(|s| {
            s.split_once(':')
                .and_then(|(v, u)| Some((v.parse().ok()?, u.parse().ok()?)))
        });
        // First sighting or UIDVALIDITY reset: baseline only.
        let Some(mark) = seen.filter(|(v, _)| *v == top.uid_validity).map(|(_, u)| u) else {
            continue;
        };
        out.extend(unread_above(db, top, mark));
    }
    (out, tops)
}

/// Record marks from [`collect_new_mail`] as seen.
pub fn commit_seen(db: &Db, marks: &[SeenMark]) {
    for m in marks {
        if let Err(e) = settings::set(db, &m.key(), &format!("{}:{}", m.uid_validity, m.uid)) {
            log::warn!("background mark for folder {} not saved: {e}", m.folder_id);
        }
    }
}

fn unread_above(db: &Db, top: &SeenMark, mark: i64) -> Vec<NewMail> {
    db.conn()
        .prepare(
            "select a.email_address, f.path, m.uid,
                        coalesce(m.from_addr, ''), coalesce(m.subject, ''),
                        coalesce(m.date, '')
                 from messages m
                 join accounts a on a.id = m.account_id
                 join folders f on f.id = m.folder_id
                 where m.folder_id = ?1 and m.is_read = 0 and m.uid > ?2
                 order by m.uid",
        )
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![top.folder_id, mark], |row| {
                Ok(NewMail {
                    account_id: top.account_id,
                    account_email: row.get(0)?,
                    folder_id: top.folder_id,
                    folder: row.get(1)?,
                    uid: row.get(2)?,
                    uid_validity: top.uid_validity as u32,
                    from: row.get(3)?,
                    subject: row.get(4)?,
                    date: row.get(5)?,
                })
            })
            .and_then(|mapped| mapped.collect::<rusqlite::Result<Vec<_>>>())
        })
        .unwrap_or_default()
}

// Cached inbox-unread total across accounts (no network), for the badge
/// when a check cannot run.
fn cached_total_unread(db: &Db) -> u64 {
    unread_summary(db).iter().map(|a| a.unread).sum()
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
        // Android has /proc too, but is not `target_os = "linux"`.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            // A live process has a /proc entry; a zombie/reaped pid does not.
            // PID reuse is harmless here: at worst we skip one background run.
            Path::new(&format!("/proc/{pid}")).exists()
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount};
    use crate::store::{accounts, folders, messages};

    fn setup_db() -> (Db, i64, i64, i64) {
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

    /// One background run whose notification succeeded.
    fn check(db: &Db) -> Vec<NewMail> {
        let (new, marks) = collect_new_mail(db);
        commit_seen(db, &marks);
        new
    }

    #[test]
    fn uncommitted_marks_report_the_same_mail_again() {
        let (db, acc, inbox, _) = setup_db();
        assert!(check(&db).is_empty());
        let mut m = messages::sample_new(acc, inbox, 3);
        m.is_read = false;
        messages::upsert(&db, &m).unwrap();
        // The post failed, so nothing was committed…
        assert_eq!(collect_new_mail(&db).0.len(), 1);
        // …and the next run offers it again.
        assert_eq!(check(&db).len(), 1);
        assert!(check(&db).is_empty());
    }

    #[test]
    fn first_mail_into_a_fully_read_inbox_is_reported() {
        let (db, acc, inbox, _) = setup_db();
        let mut read = messages::sample_new(acc, inbox, 1);
        read.is_read = true;
        messages::upsert(&db, &read).unwrap();
        assert!(check(&db).is_empty());
        let mut fresh = messages::sample_new(acc, inbox, 2);
        fresh.is_read = false;
        messages::upsert(&db, &fresh).unwrap();
        assert_eq!(check(&db).len(), 1);
    }

    #[test]
    fn first_mail_into_an_empty_inbox_is_reported() {
        let (db, acc, inbox, _) = setup_db();
        assert!(check(&db).is_empty());
        let mut fresh = messages::sample_new(acc, inbox, 1);
        fresh.is_read = false;
        messages::upsert(&db, &fresh).unwrap();
        assert_eq!(check(&db).len(), 1);
    }

    #[test]
    fn collect_new_mail_baselines_first_then_reports_once() {
        let (db, acc, inbox, _) = setup_db();
        let mut m1 = messages::sample_new(acc, inbox, 1);
        m1.is_read = false;
        m1.subject = Some("old unread".to_string());
        messages::upsert(&db, &m1).unwrap();

        // First run only records the baseline: pre-existing unread mail
        // must not notify.
        assert!(check(&db).is_empty());

        // A genuinely new arrival reports exactly once…
        let mut m2 = messages::sample_new(acc, inbox, 2);
        m2.is_read = false;
        m2.subject = Some("fresh arrival".to_string());
        messages::upsert(&db, &m2).unwrap();
        let new = check(&db);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].subject, "fresh arrival");
        assert_eq!(new[0].uid, 2);
        assert_eq!(new[0].folder_id, inbox);
        // …and the next run stays quiet while it sits unread.
        assert!(check(&db).is_empty());
    }

    #[test]
    fn collect_new_mail_rebaselines_on_uidvalidity_change() {
        let (db, acc, inbox, _) = setup_db();
        let mut m1 = messages::sample_new(acc, inbox, 7);
        m1.is_read = false;
        messages::upsert(&db, &m1).unwrap();
        assert!(check(&db).is_empty());

        // Server-side UID reset: UIDs restart low, so without the validity
        // in the mark every old message would look new.
        db.conn()
            .execute(
                "update folders set uid_validity = 99 where id = ?1",
                [inbox],
            )
            .unwrap();
        assert!(check(&db).is_empty());
        // …and mail arriving under the new validity reports normally.
        let mut m2 = messages::sample_new(acc, inbox, 8);
        m2.is_read = false;
        messages::upsert(&db, &m2).unwrap();
        assert_eq!(check(&db).len(), 1);
    }
}
