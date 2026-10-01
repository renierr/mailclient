//! Per-account overrides of app settings in `account_settings`.
//!
//! Every account inherits the app-wide value from [`settings`] until it
//! stores its own. Only the keys in [`KEYS`] can be overridden; reading one
//! through the resolvers here ([`sync_interval`], [`push_enabled`],
//! [`get_bool`]) is the only way callers should see these settings, so an
//! override can never be bypassed.

use std::collections::BTreeMap;

use rusqlite::params;
use serde::Serialize;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::store::{now, settings};

/// Keep this account connected in IMAP IDLE for push mail (`1`/`0`). It has
/// no app-wide key of its own: the default is whether the app-wide
/// background method is `push`.
pub const PUSH_ENABLED: &str = "push_enabled";

/// Prefix for the heartbeat interval last observed while the account idled
/// for push (seconds; `0` = none seen): `idle_heartbeat_secs_{account_id}`.
/// Observed state, not a preference, so it lives in `settings` and is never
/// an override.
pub const IDLE_HEARTBEAT_PREFIX: &str = "idle_heartbeat_secs_";

/// Heartbeats more frequent than this wake the phone more often than the
/// push service's own keep-alive alarm does (Android `MailPush`), so they
/// are worth a hint in the account's settings.
pub const FREQUENT_HEARTBEAT_SECS: i64 = 15 * 60;

/// The settings an account can override.
pub const KEYS: [&str; 5] = [
    settings::SYNC_INTERVAL_MINUTES,
    PUSH_ENABLED,
    settings::SENT_COPY_ENABLED,
    settings::COLLECT_SENT_CONTACTS,
    settings::NOTIFICATIONS_ENABLED,
];

/// The account's own values, keyed like [`KEYS`]. Keys it inherits are absent.
pub fn overrides(db: &Db, account_id: i64) -> Result<BTreeMap<String, String>> {
    let mut stmt = db
        .conn()
        .prepare("select key, value from account_settings where account_id = ?1")?;
    let rows = stmt.query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows
        .collect::<rusqlite::Result<BTreeMap<String, String>>>()?
        .into_iter()
        .filter(|(k, _)| KEYS.contains(&k.as_str()))
        .collect())
}

/// Store several overrides at once: all or none. An empty value removes the
/// override, so the account inherits the app-wide value again. Unknown keys
/// and unreadable values fail the whole batch.
pub fn set_overrides(db: &Db, account_id: i64, pairs: &[(String, String)]) -> Result<()> {
    let mut normalized = Vec::with_capacity(pairs.len());
    for (key, value) in pairs {
        let value = value.trim();
        let stored = if value.is_empty() {
            None
        } else {
            Some(normalize(key, value)?)
        };
        if !KEYS.contains(&key.as_str()) {
            return Err(StoreError::InvalidInput(format!(
                "not an account setting: {key}"
            )));
        }
        normalized.push((key, stored));
    }
    let ts = now();
    let tx = db.conn().unchecked_transaction()?;
    for (key, value) in normalized {
        match value {
            Some(value) => tx.execute(
                "insert into account_settings (account_id, key, value, created_at, updated_at)
                 values (?1, ?2, ?3, ?4, ?4)
                 on conflict (account_id, key)
                 do update set value = excluded.value, updated_at = excluded.updated_at",
                params![account_id, key, value, ts],
            )?,
            None => tx.execute(
                "delete from account_settings where account_id = ?1 and key = ?2",
                params![account_id, key],
            )?,
        };
    }
    tx.commit()?;
    Ok(())
}

/// The stored form of an override value, or an error for a value that does
/// not fit the key.
fn normalize(key: &str, value: &str) -> Result<String> {
    if key == settings::SYNC_INTERVAL_MINUTES {
        return value
            .parse::<i64>()
            .map(|m| settings::normalize_sync_interval(m).to_string())
            .map_err(|_| StoreError::InvalidInput(format!("{key}: not a number: {value}")));
    }
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "on" => Ok("1".to_string()),
        "0" | "false" | "off" => Ok("0".to_string()),
        _ => Err(StoreError::InvalidInput(format!(
            "{key}: not a yes/no value: {value}"
        ))),
    }
}

fn stored(db: &Db, account_id: i64, key: &str) -> Option<String> {
    db.conn()
        .query_row(
            "select value from account_settings where account_id = ?1 and key = ?2",
            params![account_id, key],
            |r| r.get(0),
        )
        .ok()
}

/// Automatic check interval for `account_id` in minutes (`0` = manually).
pub fn sync_interval(db: &Db, account_id: i64) -> i64 {
    stored(db, account_id, settings::SYNC_INTERVAL_MINUTES)
        .and_then(|v| v.parse::<i64>().ok())
        .map_or_else(
            || settings::get_sync_interval(db),
            settings::normalize_sync_interval,
        )
}

/// Whether `account_id` uses push mail (IMAP IDLE) instead of polling. Only
/// the Android background service acts on it, and only while the account
/// checks automatically at all ([`sync_interval`] above 0).
pub fn push_enabled(db: &Db, account_id: i64) -> bool {
    match stored(db, account_id, PUSH_ENABLED) {
        Some(v) => v == "1",
        None => settings::get_background_scheduler(db) == "push",
    }
}

/// A yes/no setting for `account_id`: its override, else the app-wide value.
pub fn get_bool(db: &Db, account_id: i64, key: &str) -> bool {
    if key == PUSH_ENABLED {
        return push_enabled(db, account_id);
    }
    match stored(db, account_id, key) {
        Some(v) => v == "1",
        None => settings::get_bool(db, key)
            .unwrap_or_else(|_| settings::defaults(key).is_some_and(|d| d == "1")),
    }
}

/// The server heartbeat interval (`* OK Still here`) last seen while
/// `account_id` idled for push, in seconds. `None` when it never idled or
/// the server stayed quiet.
pub fn idle_heartbeat_secs(db: &Db, account_id: i64) -> Option<i64> {
    settings::get(db, &format!("{IDLE_HEARTBEAT_PREFIX}{account_id}"))
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|secs| *secs > 0)
}

/// Record what one IDLE saw: the average heartbeat gap when the server sent
/// any, or `0` once it stayed quiet for [`FREQUENT_HEARTBEAT_SECS`]. A
/// shorter quiet IDLE (woken early) proves nothing and changes nothing.
pub fn record_idle_heartbeats(
    db: &Db,
    account_id: i64,
    heartbeats: u32,
    every_secs: Option<i64>,
    idled_secs: i64,
) {
    let value = match every_secs {
        Some(secs) if heartbeats > 0 => secs.max(1),
        _ if idled_secs >= FREQUENT_HEARTBEAT_SECS => 0,
        _ => return,
    };
    let key = format!("{IDLE_HEARTBEAT_PREFIX}{account_id}");
    if settings::get(db, &key).ok().flatten().as_deref() == Some(value.to_string().as_str()) {
        return;
    }
    if let Err(e) = settings::set(db, &key, &value.to_string()) {
        log::warn!("idle heartbeat for account {account_id} not saved: {e}");
    }
}

/// One account's settings for a settings form: what it sets itself and what
/// applies to it, both keyed like [`KEYS`] with stored string values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountSettingsView {
    pub overrides: BTreeMap<String, String>,
    pub effective: BTreeMap<String, String>,
    /// Set when the server's IDLE heartbeats come more often than
    /// [`FREQUENT_HEARTBEAT_SECS`]: the observed gap in seconds, so the form
    /// can suggest polling this account instead of push.
    pub frequent_heartbeat_secs: Option<i64>,
}

/// [`overrides`] and [`effective`] for `account_id` together.
pub fn view(db: &Db, account_id: i64) -> Result<AccountSettingsView> {
    Ok(AccountSettingsView {
        overrides: overrides(db, account_id)?,
        effective: effective(db, account_id),
        frequent_heartbeat_secs: idle_heartbeat_secs(db, account_id)
            .filter(|secs| *secs < FREQUENT_HEARTBEAT_SECS),
    })
}

/// What applies to `account_id` for every key in [`KEYS`], overridden or not.
pub fn effective(db: &Db, account_id: i64) -> BTreeMap<String, String> {
    KEYS.iter()
        .map(|&key| {
            let value = if key == settings::SYNC_INTERVAL_MINUTES {
                sync_interval(db, account_id).to_string()
            } else if get_bool(db, account_id, key) {
                "1".to_string()
            } else {
                "0".to_string()
            };
            (key.to_string(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::accounts;

    fn setup() -> (Db, i64, i64) {
        let db = Db::open_in_memory().unwrap();
        let a = accounts::create_for_test(&db, "a@example.com");
        let b = accounts::create_for_test(&db, "b@example.org");
        (db, a, b)
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn accounts_inherit_until_they_override() {
        let (db, a, b) = setup();
        settings::set_sync_interval(&db, 15).unwrap();
        settings::set_bool(&db, settings::SENT_COPY_ENABLED, false).unwrap();
        assert_eq!(sync_interval(&db, a), 15);
        assert!(!get_bool(&db, a, settings::SENT_COPY_ENABLED));

        set_overrides(
            &db,
            a,
            &pairs(&[
                (settings::SYNC_INTERVAL_MINUTES, "60"),
                (settings::SENT_COPY_ENABLED, "true"),
            ]),
        )
        .unwrap();
        assert_eq!(sync_interval(&db, a), 60);
        assert!(get_bool(&db, a, settings::SENT_COPY_ENABLED));
        assert_eq!(sync_interval(&db, b), 15, "other accounts keep the default");
        assert!(!get_bool(&db, b, settings::SENT_COPY_ENABLED));

        set_overrides(&db, a, &pairs(&[(settings::SYNC_INTERVAL_MINUTES, "")])).unwrap();
        assert_eq!(sync_interval(&db, a), 15, "an empty value inherits again");
        assert_eq!(
            overrides(&db, a).unwrap().keys().collect::<Vec<_>>(),
            vec![settings::SENT_COPY_ENABLED]
        );
    }

    #[test]
    fn push_defaults_to_the_app_wide_background_method() {
        let (db, a, b) = setup();
        assert!(!push_enabled(&db, a));
        settings::set(&db, settings::BACKGROUND_SCHEDULER, "push").unwrap();
        assert!(push_enabled(&db, a));
        set_overrides(&db, b, &pairs(&[(PUSH_ENABLED, "0")])).unwrap();
        assert!(!push_enabled(&db, b));
        assert_eq!(effective(&db, b)[PUSH_ENABLED], "0");
        assert_eq!(effective(&db, a)[PUSH_ENABLED], "1");
    }

    #[test]
    fn a_bad_batch_changes_nothing() {
        let (db, a, _) = setup();
        for bad in [
            pairs(&[
                (settings::SENT_COPY_ENABLED, "0"),
                (settings::UI_SCALE, "1"),
            ]),
            pairs(&[
                (settings::SENT_COPY_ENABLED, "0"),
                (settings::SYNC_INTERVAL_MINUTES, "soon"),
            ]),
            pairs(&[(settings::NOTIFICATIONS_ENABLED, "maybe")]),
        ] {
            assert!(set_overrides(&db, a, &bad).is_err());
        }
        assert!(overrides(&db, a).unwrap().is_empty());
    }

    #[test]
    fn intervals_are_clamped_and_overrides_go_with_the_account() {
        let (db, a, _) = setup();
        set_overrides(
            &db,
            a,
            &pairs(&[(settings::SYNC_INTERVAL_MINUTES, "99999")]),
        )
        .unwrap();
        assert_eq!(sync_interval(&db, a), 1440);
        accounts::delete(&db, a).unwrap();
        assert!(overrides(&db, a).unwrap().is_empty());
    }

    #[test]
    fn frequent_idle_heartbeats_show_until_the_server_stays_quiet() {
        let (db, a, b) = setup();
        assert_eq!(view(&db, a).unwrap().frequent_heartbeat_secs, None);

        record_idle_heartbeats(&db, a, 3, Some(120), 400);
        assert_eq!(view(&db, a).unwrap().frequent_heartbeat_secs, Some(120));
        assert_eq!(view(&db, b).unwrap().frequent_heartbeat_secs, None);

        // Woken early without a heartbeat: inconclusive, the hint stays.
        record_idle_heartbeats(&db, a, 0, None, 60);
        assert_eq!(view(&db, a).unwrap().frequent_heartbeat_secs, Some(120));

        // Rare heartbeats are remembered but not worth a hint.
        record_idle_heartbeats(&db, a, 1, Some(1200), 1500);
        assert_eq!(idle_heartbeat_secs(&db, a), Some(1200));
        assert_eq!(view(&db, a).unwrap().frequent_heartbeat_secs, None);

        record_idle_heartbeats(&db, a, 2, Some(90), 200);
        record_idle_heartbeats(&db, a, 0, None, FREQUENT_HEARTBEAT_SECS);
        assert_eq!(idle_heartbeat_secs(&db, a), None);
        assert_eq!(view(&db, a).unwrap().frequent_heartbeat_secs, None);
    }
}
