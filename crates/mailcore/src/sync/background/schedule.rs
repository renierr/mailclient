//! Which accounts the Android background mechanisms serve, and when.
//!
//! Each account checks at its own interval and either stays connected for
//! push (IMAP IDLE) or is polled (`store::account_settings`). The host runs
//! at most two mechanisms side by side: the push service for the push
//! accounts, and one poller (WorkManager or the exact alarm) ticking at the
//! shortest interval among the others. A tick only syncs the accounts that
//! are due, so a slow account is not checked at a fast one's cadence.
//!
//! An account inside its quiet hours (`account_settings::quiet_hours`, in
//! device local time) drops out of the plan entirely: no IDLE connection,
//! no poller tick on its behalf. The plan says when the next quiet window
//! starts or ends ([`BackgroundPlan::replan_at`]), and the host plans again
//! then, so nothing has to wake the phone in between.

use chrono::{DateTime, Local, TimeZone, Utc};
use serde::Serialize;

use crate::db::Db;
use crate::models::Account;
use crate::store::{account_settings, accounts, settings};

/// Prefix for the time a background tick last checked an account (unix
/// seconds; internal bookkeeping, outside the settings allowlist):
/// `bg_checked_at_{account_id}`.
pub const BG_CHECKED_PREFIX: &str = "bg_checked_at_";

/// What the Android host should run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackgroundPlan {
    /// Run the push service: at least one account checks via IDLE.
    pub push: bool,
    /// Tick the poller this often (minutes); 0 when no account is polled.
    pub poll_minutes: i64,
    /// Which poller ticks: `workmanager` or `alarm`. The app-wide `push`
    /// method polls the accounts that opted out of push with WorkManager.
    pub poll_scheduler: String,
    /// Accounts that check in the background but sit in their quiet hours
    /// now, so `push` and `poll_minutes` leave them out.
    pub quiet_accounts: usize,
    /// When some account's quiet hours next start or end (unix seconds): the
    /// host plans again then. `None` while no account has quiet hours.
    pub replan_at: Option<i64>,
}

impl BackgroundPlan {
    /// Whether any account checks in the background at all, quiet or not.
    #[must_use]
    pub fn any(&self) -> bool {
        self.push || self.poll_minutes > 0 || self.quiet_accounts > 0
    }
}

/// An account that checks in the background, and how.
struct Scheduled {
    account: Account,
    minutes: i64,
    push: bool,
    /// Inside its quiet hours at the time asked about.
    quiet: bool,
    /// When its quiet hours next start or end.
    change_at: Option<DateTime<Utc>>,
}

fn scheduled<Tz: TimeZone>(db: &Db, now: &DateTime<Tz>) -> Vec<Scheduled> {
    accounts::list(db)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|account| {
            let minutes = account_settings::sync_interval(db, account.id);
            let quiet = account_settings::quiet_hours(db, account.id);
            (minutes > 0).then(|| Scheduled {
                push: account_settings::push_enabled(db, account.id),
                quiet: quiet.is_some_and(|q| q.contains_at(now)),
                change_at: quiet.and_then(|q| q.next_change(now)),
                account,
                minutes,
            })
        })
        .collect()
}

/// The accounts that check in the background and are not quiet at `now`.
fn active<Tz: TimeZone>(db: &Db, now: &DateTime<Tz>) -> Vec<Scheduled> {
    scheduled(db, now)
        .into_iter()
        .filter(|s| !s.quiet)
        .collect()
}

/// The mechanisms the host should run now, in device local time.
pub fn plan(db: &Db) -> BackgroundPlan {
    plan_at(db, &Local::now())
}

/// [`plan`] at `now`, with quiet hours read in `now`'s time zone.
pub fn plan_at<Tz: TimeZone>(db: &Db, now: &DateTime<Tz>) -> BackgroundPlan {
    let all = scheduled(db, now);
    let replan_at = all
        .iter()
        .filter_map(|s| s.change_at)
        .min()
        .map(|t| t.timestamp());
    let quiet_accounts = all.iter().filter(|s| s.quiet).count();
    let all: Vec<Scheduled> = all.into_iter().filter(|s| !s.quiet).collect();
    let poll_minutes = all
        .iter()
        .filter(|s| !s.push)
        .map(|s| s.minutes)
        .min()
        .unwrap_or(0);
    let poll_scheduler = match settings::get_background_scheduler(db).as_str() {
        "alarm" => "alarm",
        _ => "workmanager",
    };
    BackgroundPlan {
        push: all.iter().any(|s| s.push),
        poll_minutes,
        poll_scheduler: poll_scheduler.to_string(),
        quiet_accounts,
        replan_at,
    }
}

/// Accounts the push monitor should keep in IDLE now (device local time).
pub fn push_account_ids(db: &Db) -> Vec<i64> {
    push_account_ids_at(db, &Local::now())
}

fn push_account_ids_at<Tz: TimeZone>(db: &Db, now: &DateTime<Tz>) -> Vec<i64> {
    active(db, now)
        .into_iter()
        .filter(|s| s.push)
        .map(|s| s.account.id)
        .collect()
}

/// Polled accounts whose interval has (about) run out at `now`.
///
/// Schedulers fire late and WorkManager anywhere inside its period, so an
/// account counts as due half a tick early: otherwise a 30-minute account on
/// a 15-minute tick would slip to every 45 minutes whenever a tick came a
/// little early. Accounts in their quiet hours (device local time) are
/// never due.
pub fn due_accounts(db: &Db, now: DateTime<Utc>) -> Vec<Account> {
    due_accounts_at(db, &now.with_timezone(&Local))
}

fn due_accounts_at<Tz: TimeZone>(db: &Db, now: &DateTime<Tz>) -> Vec<Account> {
    let polled: Vec<Scheduled> = active(db, now).into_iter().filter(|s| !s.push).collect();
    let tick = polled.iter().map(|s| s.minutes).min().unwrap_or(0);
    polled
        .into_iter()
        .filter(|s| {
            let Some(last) = last_checked(db, s.account.id) else {
                return true;
            };
            let elapsed = now.timestamp() - last;
            elapsed >= s.minutes * 60 - tick * 30
        })
        .map(|s| s.account)
        .collect()
}

fn last_checked(db: &Db, account_id: i64) -> Option<i64> {
    settings::get(db, &format!("{BG_CHECKED_PREFIX}{account_id}"))
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse().ok())
}

/// Record that a background tick checked `account_id` at `now`.
pub fn mark_checked(db: &Db, account_id: i64, now: DateTime<Utc>) {
    let key = format!("{BG_CHECKED_PREFIX}{account_id}");
    if let Err(e) = settings::set(db, &key, &now.timestamp().to_string()) {
        log::warn!("background check time for account {account_id} not saved: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::account_settings::{
        PUSH_ENABLED, QUIET_HOURS_ENABLED, QUIET_HOURS_END, QUIET_HOURS_START,
    };
    use chrono::{Duration, FixedOffset, NaiveDate};

    fn account(db: &Db, email: &str, overrides: &[(&str, &str)]) -> i64 {
        let id = accounts::create_for_test(db, email);
        let pairs: Vec<(String, String)> = overrides
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        account_settings::set_overrides(db, id, &pairs).unwrap();
        id
    }

    fn ids(list: &[Account]) -> Vec<i64> {
        list.iter().map(|a| a.id).collect()
    }

    #[test]
    fn manual_everywhere_runs_nothing() {
        let db = Db::open_in_memory().unwrap();
        account(&db, "a@example.com", &[]);
        assert_eq!(
            plan(&db),
            BackgroundPlan {
                push: false,
                poll_minutes: 0,
                poll_scheduler: "workmanager".into(),
                quiet_accounts: 0,
                replan_at: None,
            }
        );
    }

    #[test]
    fn push_and_polled_accounts_run_side_by_side() {
        let db = Db::open_in_memory().unwrap();
        settings::set_sync_interval(&db, 15).unwrap();
        settings::set(&db, settings::BACKGROUND_SCHEDULER, "push").unwrap();
        let quiet = account(&db, "a@example.com", &[]);
        let chatty = account(
            &db,
            "b@example.org",
            &[(PUSH_ENABLED, "0"), (settings::SYNC_INTERVAL_MINUTES, "30")],
        );
        account(
            &db,
            "c@example.org",
            &[(settings::SYNC_INTERVAL_MINUTES, "0")],
        );
        assert_eq!(
            plan(&db),
            BackgroundPlan {
                push: true,
                poll_minutes: 30,
                poll_scheduler: "workmanager".into(),
                quiet_accounts: 0,
                replan_at: None,
            }
        );
        assert_eq!(push_account_ids(&db), vec![quiet]);
        assert_eq!(ids(&due_accounts(&db, Utc::now())), vec![chatty]);
    }

    #[test]
    fn a_tick_checks_only_the_accounts_that_are_due() {
        let db = Db::open_in_memory().unwrap();
        settings::set(&db, settings::BACKGROUND_SCHEDULER, "alarm").unwrap();
        let fast = account(
            &db,
            "a@example.com",
            &[(settings::SYNC_INTERVAL_MINUTES, "15")],
        );
        let slow = account(
            &db,
            "b@example.org",
            &[(settings::SYNC_INTERVAL_MINUTES, "60")],
        );
        assert_eq!(plan(&db).poll_minutes, 15);
        assert_eq!(plan(&db).poll_scheduler, "alarm");

        let start = Utc::now();
        assert_eq!(ids(&due_accounts(&db, start)), vec![fast, slow]);
        mark_checked(&db, fast, start);
        mark_checked(&db, slow, start);

        // A tick that fires a little early still counts as on time.
        let early = start + Duration::minutes(14);
        assert_eq!(ids(&due_accounts(&db, early)), vec![fast]);
        mark_checked(&db, fast, early);

        assert!(due_accounts(&db, start + Duration::minutes(20)).is_empty());
        assert_eq!(
            ids(&due_accounts(&db, start + Duration::minutes(53))),
            vec![fast, slow]
        );
    }

    fn local(h: u32, m: u32) -> DateTime<FixedOffset> {
        let naive = NaiveDate::from_ymd_opt(2026, 3, 10)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap();
        FixedOffset::east_opt(3600)
            .unwrap()
            .from_local_datetime(&naive)
            .unwrap()
    }

    #[test]
    fn quiet_accounts_drop_out_until_their_window_ends() {
        let db = Db::open_in_memory().unwrap();
        settings::set_sync_interval(&db, 15).unwrap();
        let night = [(QUIET_HOURS_ENABLED, "1")];
        let pushed = account(&db, "a@example.com", &[night[0], (PUSH_ENABLED, "1")]);
        let polled = account(&db, "b@example.org", &night);
        let awake = account(
            &db,
            "c@example.org",
            &[(settings::SYNC_INTERVAL_MINUTES, "60")],
        );

        let at_night = plan_at(&db, &local(3, 0));
        assert!(!at_night.push, "no IDLE connection is held");
        assert_eq!(at_night.poll_minutes, 60, "only the awake account ticks");
        assert_eq!(at_night.quiet_accounts, 2);
        assert_eq!(at_night.replan_at, Some(local(7, 0).timestamp()));
        assert!(at_night.any());
        assert!(push_account_ids_at(&db, &local(3, 0)).is_empty());
        assert_eq!(ids(&due_accounts_at(&db, &local(3, 0))), vec![awake]);

        let morning = plan_at(&db, &local(7, 0));
        assert!(morning.push);
        assert_eq!(morning.poll_minutes, 15);
        assert_eq!(morning.quiet_accounts, 0);
        assert_eq!(
            morning.replan_at,
            Some((local(0, 0) + Duration::days(1)).timestamp())
        );
        assert_eq!(push_account_ids_at(&db, &local(7, 0)), vec![pushed]);
        assert_eq!(
            ids(&due_accounts_at(&db, &local(7, 0))),
            vec![polled, awake]
        );
    }

    #[test]
    fn the_earliest_window_change_decides_the_replan() {
        let db = Db::open_in_memory().unwrap();
        settings::set_sync_interval(&db, 15).unwrap();
        account(&db, "a@example.com", &[(QUIET_HOURS_ENABLED, "1")]);
        account(
            &db,
            "b@example.org",
            &[
                (QUIET_HOURS_ENABLED, "1"),
                (QUIET_HOURS_START, "22:00"),
                (QUIET_HOURS_END, "06:00"),
            ],
        );
        // Manual accounts never check in the background, quiet or not.
        account(
            &db,
            "c@example.org",
            &[
                (settings::SYNC_INTERVAL_MINUTES, "0"),
                (QUIET_HOURS_ENABLED, "1"),
                (QUIET_HOURS_START, "12:30"),
            ],
        );
        let evening = plan_at(&db, &local(21, 0));
        assert_eq!(evening.replan_at, Some(local(22, 0).timestamp()));
        assert_eq!(evening.quiet_accounts, 0);
        let late = plan_at(&db, &local(23, 0));
        assert_eq!(late.quiet_accounts, 1);
        assert_eq!(
            late.replan_at,
            Some((local(0, 0) + Duration::days(1)).timestamp())
        );
    }
}
