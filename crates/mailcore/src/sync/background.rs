//! Android background mail check: the headless tick the Flutter WorkManager
//! worker runs while the app is closed.
//!
//! A tick has to fit into Doze's short maintenance windows, so it syncs the
//! inbox only ([`SyncScope::InboxOnly`]) over the cached folder tree, then
//! reports mail above per-folder high-water marks. Every tick is kept in a
//! short [`LastRun`] history so the UI can show whether Android actually ran
//! it, which scheduler did, and what became of the notification.
//!
//! [`push_check`] is the same check for one account over the connection the
//! IDLE monitor (`sync::push`) already holds; [`notify`] turns a report into
//! what the notification should do.

pub mod notify;
pub mod schedule;

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::models::Account;
use crate::store::settings;
use crate::sync::headless::{
    self, acquire_sync_lock, sync_account, sync_accounts, SyncLock, SyncScope,
};
use crate::sync::imap::ImapSync;

/// One mail the background check has never reported before. Carries the ids
/// the UI needs to open it (`account_id`/`folder_id`/`uid`), plus the
/// metadata a notification shows. Bodies never cross here, only the cached
/// list snippet the expanded notification previews.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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
    pub snippet: String,
}

/// Outcome of [`background_check`]: what arrived since the previous check.
/// Crosses to the Android host as JSON and back into [`notify::plan`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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
    /// Everything the notification should list: unread inbox mail that
    /// arrived since the user last had the app open ([`mark_inbox_seen`]).
    /// A superset of `new`, so a second arrival does not push the first out
    /// of a notification the user has not looked at yet.
    pub pending: Vec<NewMail>,
    /// `started_at` of this tick's [`LastRun`], for [`record_outcome`].
    pub run: String,
    /// Cached inbox-unread total across accounts, for the launcher badge.
    pub total_unread: u64,
    pub errors: Vec<String>,
}

/// Prefix for background-check high-water marks (internal bookkeeping, not
/// a user preference, so deliberately outside the settings allowlist):
/// `bg_seen_uid_{account_id}_{folder_id}` stores `{uidvalidity}:{max_uid}`.
pub const BG_SEEN_PREFIX: &str = "bg_seen_uid_";

/// Same shape as [`BG_SEEN_PREFIX`], but moved only while the user has the
/// app open: the line above which mail is still unseen for the
/// notification's list.
pub const BG_SHOWN_PREFIX: &str = "bg_shown_uid_";

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

    fn shown_key(&self) -> String {
        format!("{BG_SHOWN_PREFIX}{}_{}", self.account_id, self.folder_id)
    }

    fn value(&self) -> String {
        format!("{}:{}", self.uid_validity, self.uid)
    }
}

/// Settings key holding the [`LastRun`] history as a JSON array, newest
/// first (internal bookkeeping, not a user preference, so outside the
/// settings allowlist).
pub const BG_RUN_HISTORY: &str = "bg_run_history";

/// Older single-record key, still read while no history exists.
const BG_LAST_RUN: &str = "bg_last_run";

/// How many ticks the history keeps.
pub const RUN_HISTORY_LEN: usize = 10;

/// Record of one background tick. `finished_at` stays `None` while a tick
/// runs — or for good, when Android stopped it mid-run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastRun {
    pub started_at: String,
    pub finished_at: Option<String>,
    /// Which scheduler started the tick (`worker`, `alarm`), as the caller
    /// named it.
    #[serde(default)]
    pub trigger: String,
    pub skipped: bool,
    pub new: usize,
    /// First few errors only; the full list is in the report.
    pub errors: Vec<String>,
    /// What the caller did with the report (posted, alerts off, post
    /// failed, …), set by [`record_outcome`].
    #[serde(default)]
    pub outcome: Option<String>,
}

/// How many errors [`LastRun`] keeps.
const LAST_RUN_ERRORS: usize = 3;

impl LastRun {
    fn started(at: DateTime<Utc>, trigger: &str) -> Self {
        LastRun {
            started_at: at.to_rfc3339(),
            trigger: trigger.to_string(),
            ..Default::default()
        }
    }

    fn finish(mut self, at: DateTime<Utc>, report: &BackgroundReport) -> Self {
        self.finished_at = Some(at.to_rfc3339());
        self.skipped = report.skipped;
        self.new = report.new.len();
        self.errors = report
            .errors
            .iter()
            .take(LAST_RUN_ERRORS)
            .cloned()
            .collect();
        self
    }
}

/// Store `run` in the history: it replaces the entry of the same tick (same
/// `started_at`), or goes in front as the newest.
fn save_run(db: &Db, run: &LastRun) {
    update_history(db, |history| {
        match history.iter_mut().find(|r| r.started_at == run.started_at) {
            Some(slot) => *slot = run.clone(),
            None => history.insert(0, run.clone()),
        }
    });
}

fn update_history(db: &Db, f: impl FnOnce(&mut Vec<LastRun>)) {
    let mut history = run_history(db);
    f(&mut history);
    history.truncate(RUN_HISTORY_LEN);
    let saved = serde_json::to_string(&history)
        .map_err(|e| e.to_string())
        .and_then(|json| settings::set(db, BG_RUN_HISTORY, &json).map_err(|e| e.to_string()));
    if let Err(e) = saved {
        log::warn!("background run history not saved: {e}");
    }
}

/// The recorded ticks, newest first; empty before the first one ran.
#[must_use]
pub fn run_history(db: &Db) -> Vec<LastRun> {
    let stored = |key| settings::get(db, key).ok().flatten();
    if let Some(list) = stored(BG_RUN_HISTORY).and_then(|s| serde_json::from_str(&s).ok()) {
        return list;
    }
    stored(BG_LAST_RUN)
        .and_then(|s| serde_json::from_str(&s).ok())
        .into_iter()
        .collect()
}

/// The last recorded tick, if any ran yet.
#[must_use]
pub fn last_run(db: &Db) -> Option<LastRun> {
    run_history(db).into_iter().next()
}

/// Record a check that could not even start (the push monitor failed to
/// connect), so the history shows why no checks are running.
pub fn record_failed_run(db: &Db, trigger: &str, error: &str) {
    let now = Utc::now();
    let run = LastRun {
        finished_at: Some(now.to_rfc3339()),
        errors: vec![error.to_string()],
        ..LastRun::started(now, trigger)
    };
    save_run(db, &run);
}

/// Note what became of the report of the tick that started at `started_at`.
/// A tick that already fell out of the history is ignored.
pub fn record_outcome(db: &Db, started_at: &str, outcome: &str) {
    update_history(db, |history| {
        if let Some(run) = history.iter_mut().find(|r| r.started_at == started_at) {
            run.outcome = Some(outcome.to_string());
        }
    });
}

/// One background tick: lock, sync the inbox of every polled account that
/// is due ([`schedule::due_accounts`]) over fresh connections, and report
/// mail that arrived since the previous tick.
/// Every tick, skipped or not, leaves a [`LastRun`] tagged with `trigger`.
///
/// The lock turns overlap with a foreground sync (or a second worker) into
/// a quiet skip rather than a `database is locked` error. Fresh connections
/// keep this off the GUI's pooled sessions, so a worker run can never steal
/// or stall the session the user is reading through.
pub async fn background_check(db: &Db, db_path: &Path, trigger: &str) -> BackgroundReport {
    let run = LastRun::started(Utc::now(), trigger);
    save_run(db, &run);
    let mut report = background_tick(db, db_path).await;
    report.run = run.started_at.clone();
    save_run(db, &run.finish(Utc::now(), &report));
    report
}

async fn background_tick(db: &Db, db_path: &Path) -> BackgroundReport {
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
    let started = Utc::now();
    let due = schedule::due_accounts(db, started);
    let report = sync_accounts(db, &due, SyncScope::InboxOnly).await;
    for result in report.accounts.iter().filter(|r| r.errors.is_empty()) {
        schedule::mark_checked(db, result.account_id, started);
    }
    let (new, marks) = collect_new_mail(db);
    BackgroundReport {
        skipped: false,
        new,
        marks,
        pending: collect_pending(db),
        total_unread: cached_total_unread(db),
        errors: report.errors,
        run: String::new(),
    }
}

/// How often [`push_check`] retries for the sync lock, and how long it waits
/// between tries. Only a scheduled check or the headless CLI holds the lock,
/// and neither runs for long.
const PUSH_LOCK_TRIES: u32 = 4;
const PUSH_LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(15);

/// The check the IDLE monitor runs when the server announced a change to
/// `account`'s inbox: sync that inbox over `imap` (the monitor's open
/// session, so no reconnect), then report like [`background_check`]. Logged
/// in the run history as trigger `push`.
///
/// Leaves the inbox selected, which is what the next IDLE needs. Errors in
/// the report usually mean the session broke; the caller reconnects then.
pub async fn push_check(
    db: &Db,
    db_path: &Path,
    account: &Account,
    imap: &mut ImapSync,
) -> BackgroundReport {
    let run = LastRun::started(Utc::now(), "push");
    save_run(db, &run);
    let mut report = match push_lock(db_path).await {
        Ok(Some(_lock)) => {
            let result = sync_account(db, account, imap, SyncScope::InboxOnly, None).await;
            let (new, marks) = collect_new_mail(db);
            BackgroundReport {
                skipped: false,
                new,
                marks,
                pending: collect_pending(db),
                total_unread: cached_total_unread(db),
                errors: result
                    .errors
                    .iter()
                    .map(|e| format!("{}: {e}", account.email_address))
                    .collect(),
                run: String::new(),
            }
        }
        Ok(None) => BackgroundReport {
            skipped: true,
            total_unread: cached_total_unread(db),
            ..Default::default()
        },
        Err(e) => BackgroundReport {
            skipped: true,
            total_unread: cached_total_unread(db),
            errors: vec![format!("sync lock: {e}")],
            ..Default::default()
        },
    };
    report.run = run.started_at.clone();
    save_run(db, &run.finish(Utc::now(), &report));
    report
}

async fn push_lock(db_path: &Path) -> crate::error::Result<Option<SyncLock>> {
    for attempt in 1..=PUSH_LOCK_TRIES {
        if let Some(lock) = acquire_sync_lock(db_path)? {
            return Ok(Some(lock));
        }
        if attempt < PUSH_LOCK_TRIES {
            tokio::time::sleep(PUSH_LOCK_WAIT).await;
        }
    }
    Ok(None)
}

/// Synchronous wrapper around [`background_check`] for FFI callers, which
/// cannot await. Same current-thread runtime shape as
/// [`sync_all_accounts_blocking`].
pub fn background_check_blocking(db: &Db, db_path: &Path, trigger: &str) -> BackgroundReport {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime for background check");
    rt.block_on(background_check(db, db_path, trigger))
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
    let tops = inbox_tops(db);
    let mut out = Vec::new();
    for top in &tops {
        // First sighting or UIDVALIDITY reset: baseline only.
        let Some(mark) = stored_mark(db, &top.key(), top.uid_validity) else {
            continue;
        };
        out.extend(unread_above(db, top, mark));
    }
    (out, tops)
}

/// Unread inbox mail above the "shown" line [`mark_inbox_seen`] draws while
/// the user has the app open: what the notification lists. [`commit_seen`]
/// draws a first line where none exists; until then the seen mark stands in,
/// so this never lists more than [`collect_new_mail`] would have.
pub fn collect_pending(db: &Db) -> Vec<NewMail> {
    let mut out = Vec::new();
    for top in &inbox_tops(db) {
        let mark = stored_mark(db, &top.shown_key(), top.uid_validity)
            .or_else(|| stored_mark(db, &top.key(), top.uid_validity));
        if let Some(mark) = mark {
            out.extend(unread_above(db, top, mark));
        }
    }
    out
}

/// The user has the app open: everything in the inbox cache now counts as
/// seen, for the next alert and for the notification's list. No network.
pub fn mark_inbox_seen(db: &Db) {
    for top in &inbox_tops(db) {
        for key in [top.key(), top.shown_key()] {
            if let Err(e) = settings::set(db, &key, &top.value()) {
                log::warn!(
                    "background mark for folder {} not saved: {e}",
                    top.folder_id
                );
            }
        }
    }
}

/// A stored `{uidvalidity}:{uid}` mark, if present and still under `validity`.
fn stored_mark(db: &Db, key: &str, validity: i64) -> Option<i64> {
    let raw = settings::get(db, key).ok().flatten()?;
    let (v, u) = raw.split_once(':')?;
    if v.parse::<i64>().ok()? != validity {
        return None;
    }
    u.parse().ok()
}

/// Each inbox folder's current top: its UIDVALIDITY and highest cached UID,
/// read or not (0 for an empty inbox).
fn inbox_tops(db: &Db) -> Vec<SeenMark> {
    db.conn()
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
        .unwrap_or_default()
}

/// Record marks from [`collect_new_mail`] as seen.
///
/// A folder without a valid shown line gets one here, at the seen mark this
/// commit moves past (or the baseline on a first sighting): from then on the
/// notification's list starts where alerts started, until the app is opened.
pub fn commit_seen(db: &Db, marks: &[SeenMark]) {
    for m in marks {
        if stored_mark(db, &m.shown_key(), m.uid_validity).is_none() {
            let line = SeenMark {
                uid: stored_mark(db, &m.key(), m.uid_validity).unwrap_or(m.uid),
                ..m.clone()
            };
            if let Err(e) = settings::set(db, &m.shown_key(), &line.value()) {
                log::warn!(
                    "background shown line for folder {} not saved: {e}",
                    m.folder_id
                );
            }
        }
        if let Err(e) = settings::set(db, &m.key(), &m.value()) {
            log::warn!("background mark for folder {} not saved: {e}", m.folder_id);
        }
    }
}

fn unread_above(db: &Db, top: &SeenMark, mark: i64) -> Vec<NewMail> {
    db.conn()
        .prepare(
            "select a.email_address, f.path, m.uid,
                        coalesce(m.from_addr, ''), coalesce(m.subject, ''),
                        coalesce(m.date, ''), coalesce(m.snippet, '')
                 from messages m
                 join accounts a on a.id = m.account_id
                 join folders f on f.id = m.folder_id
                 where m.folder_id = ?1 and m.is_read = 0 and m.uid > ?2
                   and m.id not in (select message_id from pending_moves)
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
                    snippet: row.get(6)?,
                })
            })
            .and_then(|mapped| mapped.collect::<rusqlite::Result<Vec<_>>>())
        })
        .unwrap_or_default()
}

/// Cached inbox-unread total across accounts (no network), for the badge
/// when a check cannot run.
fn cached_total_unread(db: &Db) -> u64 {
    headless::unread_summary(db).iter().map(|a| a.unread).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::messages;
    use crate::sync::headless::tests::setup_db;

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

    #[test]
    fn a_tick_records_its_start_and_finish() {
        // No accounts: the tick stays offline and still leaves its record.
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        assert!(last_run(&db).is_none());
        let report = background_check_blocking(&db, &dir.path().join("db.sqlite"), "worker");
        let run = last_run(&db).unwrap();
        assert!(!run.skipped);
        assert!(run.finished_at.is_some());
        assert_eq!(run.new, report.new.len());
        assert_eq!(run.trigger, "worker");
        assert_eq!(report.run, run.started_at);
    }

    #[test]
    fn a_skipped_tick_is_recorded_as_skipped() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let _held = acquire_sync_lock(&path).unwrap().unwrap();
        assert!(background_check_blocking(&db, &path, "alarm").skipped);
        let run = last_run(&db).unwrap();
        assert!(run.skipped);
        assert!(run.finished_at.is_some());
    }

    #[test]
    fn last_run_keeps_only_the_first_errors() {
        let report = BackgroundReport {
            errors: (0..10).map(|i| format!("e{i}")).collect(),
            ..Default::default()
        };
        let run = LastRun::started(Utc::now(), "worker").finish(Utc::now(), &report);
        assert_eq!(run.errors, ["e0", "e1", "e2"]);
    }

    fn unread(db: &Db, acc: i64, inbox: i64, uid: u32) {
        let mut m = messages::sample_new(acc, inbox, uid);
        m.is_read = false;
        messages::upsert(db, &m).unwrap();
    }

    #[test]
    fn pending_keeps_unseen_mail_until_the_app_is_opened() {
        let (db, acc, inbox, _) = setup_db();
        assert!(check(&db).is_empty());
        unread(&db, acc, inbox, 1);
        assert_eq!(check(&db).len(), 1);
        unread(&db, acc, inbox, 2);
        // Only the second arrival alerts, but the notification lists both.
        let (new, marks) = collect_new_mail(&db);
        commit_seen(&db, &marks);
        assert_eq!(new.iter().map(|m| m.uid).collect::<Vec<_>>(), [2]);
        let pending: Vec<u32> = collect_pending(&db).iter().map(|m| m.uid).collect();
        assert_eq!(pending, [1, 2]);
        // Opening the app clears both.
        mark_inbox_seen(&db);
        assert!(collect_pending(&db).is_empty());
        assert!(check(&db).is_empty());
    }

    #[test]
    fn mail_seen_in_the_app_does_not_alert_later() {
        let (db, acc, inbox, _) = setup_db();
        assert!(check(&db).is_empty());
        // Synced by the foreground while the user had the app open.
        unread(&db, acc, inbox, 1);
        mark_inbox_seen(&db);
        assert!(check(&db).is_empty());
        assert!(collect_pending(&db).is_empty());
    }

    #[test]
    fn history_keeps_the_newest_runs_with_their_outcome() {
        let db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.sqlite");
        let mut runs = Vec::new();
        for _ in 0..RUN_HISTORY_LEN + 2 {
            runs.push(background_check_blocking(&db, &path, "alarm").run);
        }
        let history = run_history(&db);
        assert_eq!(history.len(), RUN_HISTORY_LEN);
        let last = runs.last().unwrap();
        assert_eq!(&history[0].started_at, last);
        record_outcome(&db, last, "posted 2");
        assert_eq!(last_run(&db).unwrap().outcome.as_deref(), Some("posted 2"));
        assert_eq!(run_history(&db).len(), RUN_HISTORY_LEN);
    }

    #[test]
    fn history_falls_back_to_the_old_single_record() {
        let db = Db::open_in_memory().unwrap();
        settings::set(
            &db,
            BG_LAST_RUN,
            r#"{"started_at":"2020-01-01T00:00:00Z","finished_at":null,"skipped":false,"new":0,"errors":[]}"#,
        )
        .unwrap();
        let run = last_run(&db).unwrap();
        assert_eq!(run.started_at, "2020-01-01T00:00:00Z");
        assert_eq!(run.trigger, "");
    }
}
