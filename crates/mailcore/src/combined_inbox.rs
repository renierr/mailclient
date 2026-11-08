//! A local view over the subscribed inboxes of opted-in accounts.
//!
//! This creates no IMAP folder and moves no messages. Every row retains its
//! actual account, folder and UID so actions can target the original message.

use std::collections::HashMap;

use serde::Serialize;

use crate::db::Db;
use crate::error::Result;
use crate::feed;
use crate::models::{Account, FolderRole};
use crate::store::{account_settings, accounts, folders, messages, settings};

/// Subscribed inbox folders for the opted-in accounts, in account order.
pub fn folder_ids(db: &Db) -> Result<Vec<i64>> {
    let mut ids = Vec::new();
    for account in included_accounts(db)? {
        ids.extend(
            folders::list_by_account(db, account.id)?
                .into_iter()
                .filter(|f| f.subscribed && f.role == FolderRole::Inbox)
                .map(|f| f.id),
        );
    }
    Ok(ids)
}

/// Accounts selected by the user, including those awaiting inbox discovery.
pub fn included_accounts(db: &Db) -> Result<Vec<Account>> {
    Ok(accounts::list(db)?
        .into_iter()
        .filter(|a| account_settings::combined_inbox(db, a.id))
        .collect())
}

/// Explicit foreground refresh, continuing if one account is unavailable.
pub async fn sync(db: &Db) -> Result<crate::sync::headless::SyncAllReport> {
    let accounts = included_accounts(db)?;
    Ok(crate::sync::headless::sync_accounts(
        db,
        &accounts,
        crate::sync::headless::SyncScope::InboxOnly,
    )
    .await)
}

/// Status for an explicit refresh; errors remain visible when others succeed.
pub fn sync_status(report: &crate::sync::headless::SyncAllReport) -> String {
    let base = format!(
        "Synced {} accounts: +{} new, -{} removed",
        report.accounts.len(),
        report.total_fetched,
        report.total_expunged
    );
    match report.errors.first() {
        Some(first) => format!("{base}; {} error(s): {first}", report.errors.len()),
        None => base,
    }
}

/// Counts cover the local cache, excluding messages awaiting a move.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub total: u64,
    pub unread: u64,
    pub inboxes: u64,
}

pub fn counts(db: &Db) -> Result<Counts> {
    let mut counts = Counts::default();
    let mut account_ids = std::collections::HashSet::new();
    for id in folder_ids(db)? {
        account_ids.insert(folders::get(db, id)?.account_id);
        counts.total += messages::count_by_folder(db, id)?;
        counts.unread += messages::count_unread(db, id)?;
    }
    counts.inboxes = account_ids.len() as u64;
    Ok(counts)
}

/// A globally sorted page of locally cached messages, attributed to accounts.
pub fn messages_json(db: &Db, limit: u64, offset: u64) -> Result<String> {
    let ids = folder_ids(db)?;
    let rows = messages::list_compact_by_folders_sorted(
        db,
        &ids,
        limit,
        offset,
        &settings::get_sort_field(db),
        settings::get_sort_descending(db),
    )?;
    let accounts: HashMap<_, _> = accounts::list(db)?.into_iter().map(|a| (a.id, a)).collect();
    let paths: HashMap<_, _> = ids
        .iter()
        .filter_map(|id| folders::get(db, *id).ok().map(|f| (f.id, f.path)))
        .collect();
    let rows: Vec<_> = rows
        .into_iter()
        .map(|m| {
            let account = accounts.get(&m.account_id);
            let folder = paths.get(&m.folder_id);
            let mut row = feed::list_row_json(m, false);
            row["folder"] = serde_json::json!(folder.map(String::as_str).unwrap_or(""));
            row["account_name"] = serde_json::json!(account.map(|a| a.name.as_str()).unwrap_or(""));
            row["account_email"] =
                serde_json::json!(account.map(|a| a.email_address.as_str()).unwrap_or(""));
            row
        })
        .collect();
    Ok(serde_json::to_string(&rows)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::pending_moves::{self, PendingAction};

    fn opt_in(db: &Db, account: i64) {
        account_settings::set_overrides(
            db,
            account,
            &[(settings::COMBINED_INBOX.into(), "1".into())],
        )
        .unwrap();
    }

    #[test]
    fn membership_counts_and_rows_follow_real_inboxes() {
        let db = Db::open_in_memory().unwrap();
        let a = accounts::create_for_test(&db, "a@example.com");
        let b = accounts::create_for_test(&db, "b@example.org");
        let fa = folders::upsert(&db, a, "INBOX", "/", FolderRole::Inbox).unwrap();
        let fb = folders::upsert(&db, b, "INBOX", "/", FolderRole::Inbox).unwrap();
        let archive = folders::upsert(&db, a, "Archive", "/", FolderRole::Archive).unwrap();
        let mut first = messages::sample_new(a, fa, 7);
        first.date = Some("2026-01-01T10:00:00Z".into());
        messages::upsert(&db, &first).unwrap();
        let mut second = messages::sample_new(b, fb, 7);
        second.date = Some("2026-01-02T10:00:00Z".into());
        second.is_read = true;
        let second_id = messages::upsert(&db, &second).unwrap();
        messages::upsert(&db, &messages::sample_new(a, archive, 8)).unwrap();

        assert_eq!(counts(&db).unwrap(), Counts::default());
        opt_in(&db, a);
        opt_in(&db, b);
        assert_eq!(
            counts(&db).unwrap(),
            Counts {
                total: 2,
                unread: 1,
                inboxes: 2
            }
        );
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&messages_json(&db, 1, 0).unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["account_id"], b);
        assert_eq!(rows[0]["folder_id"], fb);
        assert_eq!(rows[0]["uid"], 7);
        let next: Vec<serde_json::Value> =
            serde_json::from_str(&messages_json(&db, 1, 1).unwrap()).unwrap();
        assert_eq!(next[0]["folder_id"], fa);

        pending_moves::queue(
            &db,
            &[second_id],
            PendingAction::Archive,
            None,
            "batch",
            "2099-01-01T00:00:00Z",
        )
        .unwrap();
        assert_eq!(
            counts(&db).unwrap(),
            Counts {
                total: 1,
                unread: 1,
                inboxes: 2
            }
        );
        folders::set_subscribed(&db, fa, false).unwrap();
        assert_eq!(
            counts(&db).unwrap(),
            Counts {
                total: 0,
                unread: 0,
                inboxes: 1
            }
        );
        accounts::delete(&db, b).unwrap();
        assert!(folder_ids(&db).unwrap().is_empty());
    }

    #[test]
    fn local_search_is_limited_to_opted_in_inboxes() {
        let db = Db::open_in_memory().unwrap();
        let a = accounts::create_for_test(&db, "a@example.com");
        let b = accounts::create_for_test(&db, "b@example.org");
        let fa = folders::upsert(&db, a, "INBOX", "/", FolderRole::Inbox).unwrap();
        let fb = folders::upsert(&db, b, "INBOX", "/", FolderRole::Inbox).unwrap();
        let archive = folders::upsert(&db, a, "Archive", "/", FolderRole::Archive).unwrap();
        for (acc, folder) in [(a, fa), (b, fb), (a, archive)] {
            messages::upsert(&db, &messages::sample_new(acc, folder, 1)).unwrap();
        }
        opt_in(&db, a);
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&feed::combined_inbox_search_json(&db, "Hello", 20).unwrap())
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["account_id"], a);
        assert_eq!(rows[0]["folder_id"], fa);
        opt_in(&db, b);
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&feed::combined_inbox_search_json(&db, "is:unread", 20).unwrap())
                .unwrap();
        assert_eq!(rows.len(), 2);
        folders::set_subscribed(&db, fa, false).unwrap();
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&feed::combined_inbox_search_json(&db, "Hello", 20).unwrap())
                .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["account_id"], b);
    }

    #[test]
    fn opt_in_is_account_only_and_can_be_removed() {
        let db = Db::open_in_memory().unwrap();
        let a = accounts::create_for_test(&db, "a@example.com");
        let b = accounts::create_for_test(&db, "b@example.org");
        assert!(!account_settings::combined_inbox(&db, a));
        // Even a manually written app-wide key is not a global opt-in.
        settings::set(&db, settings::COMBINED_INBOX, "1").unwrap();
        assert!(!account_settings::combined_inbox(&db, a));
        opt_in(&db, a);
        assert!(account_settings::combined_inbox(&db, a));
        assert!(!account_settings::combined_inbox(&db, b));
        account_settings::set_overrides(&db, a, &[(settings::COMBINED_INBOX.into(), "".into())])
            .unwrap();
        assert!(!account_settings::combined_inbox(&db, a));
    }

    #[test]
    fn sorting_and_paging_are_global_and_stable_with_duplicate_uids() {
        let db = Db::open_in_memory().unwrap();
        let a = accounts::create_for_test(&db, "a@example.com");
        let b = accounts::create_for_test(&db, "b@example.org");
        let fa = folders::upsert(&db, a, "INBOX", "/", FolderRole::Inbox).unwrap();
        let fb = folders::upsert(&db, b, "INBOX", "/", FolderRole::Inbox).unwrap();
        opt_in(&db, a);
        opt_in(&db, b);
        for (account, folder, subject) in [(a, fa, "Zulu"), (b, fb, "alpha")] {
            let mut msg = messages::sample_new(account, folder, 1);
            msg.subject = Some(subject.into());
            messages::upsert(&db, &msg).unwrap();
        }
        settings::set_sort(&db, "subject", false).unwrap();
        let first: Vec<serde_json::Value> =
            serde_json::from_str(&messages_json(&db, 1, 0).unwrap()).unwrap();
        let second: Vec<serde_json::Value> =
            serde_json::from_str(&messages_json(&db, 1, 1).unwrap()).unwrap();
        assert_eq!(first[0]["folder_id"], fb);
        assert_eq!(second[0]["folder_id"], fa);
        settings::set_sort(&db, "date", true).unwrap();
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&messages_json(&db, 2, 0).unwrap()).unwrap();
        assert_eq!(rows[0]["folder_id"], fb);
        assert_eq!(rows[1]["folder_id"], fa);
    }

    #[test]
    fn account_count_is_distinct_and_refresh_reports_partial_failure() {
        let db = Db::open_in_memory().unwrap();
        let account = accounts::create_for_test(&db, "a@example.com");
        folders::upsert(&db, account, "INBOX", "/", FolderRole::Inbox).unwrap();
        folders::upsert(&db, account, "Other/Inbox", "/", FolderRole::Inbox).unwrap();
        opt_in(&db, account);
        assert_eq!(counts(&db).unwrap().inboxes, 1);
        assert_eq!(included_accounts(&db).unwrap().len(), 1);
        let report = crate::sync::headless::SyncAllReport {
            accounts: vec![crate::sync::headless::AccountSyncResult::default()],
            total_fetched: 3,
            errors: vec!["offline".into()],
            ..Default::default()
        };
        assert_eq!(
            sync_status(&report),
            "Synced 1 accounts: +3 new, -0 removed; 1 error(s): offline"
        );
    }
}
