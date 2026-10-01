//! Which accounts the Android background mechanisms serve, and when.
//!
//! Each account checks at its own interval and either stays connected for
//! push (IMAP IDLE) or is polled (`store::account_settings`). The host runs
//! at most two mechanisms side by side: the push service for the push
//! accounts, and one poller (WorkManager or the exact alarm) ticking at the
//! shortest interval among the others. A tick only syncs the accounts that
//! are due, so a slow account is not checked at a fast one's cadence.

use chrono::{DateTime, Utc};
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
}

/// An account that checks in the background, and how.
struct Scheduled {
    account: Account,
    minutes: i64,
    push: bool,
}

fn scheduled(db: &Db) -> Vec<Scheduled> {
    accounts::list(db)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|account| {
            let minutes = account_settings::sync_interval(db, account.id);
            (minutes > 0).then(|| Scheduled {
                push: account_settings::push_enabled(db, account.id),
                account,
                minutes,
            })
        })
        .collect()
}

/// The mechanisms the host should run for the current settings.
pub fn plan(db: &Db) -> BackgroundPlan {
    let all = scheduled(db);
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
    }
}

/// Accounts the push monitor should keep in IDLE.
pub fn push_account_ids(db: &Db) -> Vec<i64> {
    scheduled(db)
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
/// little early.
pub fn due_accounts(db: &Db, now: DateTime<Utc>) -> Vec<Account> {
    let polled: Vec<Scheduled> = scheduled(db).into_iter().filter(|s| !s.push).collect();
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
    use crate::store::account_settings::PUSH_ENABLED;
    use chrono::Duration;

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
}
