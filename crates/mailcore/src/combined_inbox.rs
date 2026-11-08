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

/// Queue a combined-inbox delete without silently escalating a Trash move
/// into a permanent delete if the folder configuration changed meanwhile.
pub fn queue_delete(
    db: &Db,
    account_id: i64,
    folder_id: i64,
    uid: u32,
    confirmed_permanent: bool,
) -> std::result::Result<crate::undo::Queued, String> {
    if uid == 0 {
        return Err("invalid message".into());
    }
    let queued = crate::undo::queue_move(
        db,
        account_id,
        folder_id,
        &[uid],
        crate::undo::MoveTarget::Trash,
    )?;
    if matches!(queued, crate::undo::Queued::Permanent) && !confirmed_permanent {
        return Err("Trash is no longer available; refresh and confirm permanent deletion".into());
    }
    Ok(queued)
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
    let rows = rows
        .into_iter()
        .map(|m| feed::list_row_json(m, false))
        .collect();
    decorate_rows(db, rows)
}

/// Local search hits with the same account and action metadata as list rows.
pub fn search_json(db: &Db, query: &str, limit: u64) -> Result<String> {
    let rows = serde_json::from_str(&feed::combined_inbox_search_json(db, query, limit)?)?;
    decorate_rows(db, rows)
}

fn decorate_rows(db: &Db, mut rows: Vec<serde_json::Value>) -> Result<String> {
    let mut metadata = HashMap::new();
    for account in included_accounts(db)? {
        let all = folders::list_by_account(db, account.id)?;
        for folder in &all {
            if folder.subscribed && folder.role == FolderRole::Inbox {
                metadata.insert(
                    folder.id,
                    (
                        account.name.clone(),
                        account.email_address.clone(),
                        folder.path.clone(),
                        crate::undo::delete_is_permanent(folder, &all),
                    ),
                );
            }
        }
    }
    for row in &mut rows {
        let Some((name, email, path, permanent)) =
            row["folder_id"].as_i64().and_then(|id| metadata.get(&id))
        else {
            continue;
        };
        row["folder"] = serde_json::json!(path);
        row["account_name"] = serde_json::json!(name);
        row["account_email"] = serde_json::json!(email);
        row["delete_is_permanent"] = serde_json::json!(permanent);
    }
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
        assert_eq!(rows[0]["account_email"], "b@example.org");
        assert_eq!(rows[0]["delete_is_permanent"], true);
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
            serde_json::from_str(&search_json(&db, "Hello", 20).unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["account_id"], a);
        assert_eq!(rows[0]["folder_id"], fa);
        assert_eq!(rows[0]["account_email"], "a@example.com");
        assert_eq!(rows[0]["delete_is_permanent"], true);
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
    fn row_identity_and_delete_policy_stay_with_the_originating_account() {
        use crate::undo::{self, MoveTarget, Queued};

        let db = Db::open_in_memory().unwrap();
        let a = accounts::create_for_test(&db, "a@example.com");
        let b = accounts::create_for_test(&db, "b@example.org");
        let fa = folders::upsert(&db, a, "INBOX", "/", FolderRole::Inbox).unwrap();
        let fb = folders::upsert(&db, b, "INBOX", "/", FolderRole::Inbox).unwrap();
        let trash = folders::upsert(&db, a, "Trash", "/", FolderRole::Trash).unwrap();
        opt_in(&db, a);
        opt_in(&db, b);
        messages::upsert(&db, &messages::sample_new(a, fa, 7)).unwrap();
        messages::upsert(&db, &messages::sample_new(b, fb, 7)).unwrap();

        for json in [
            messages_json(&db, 10, 0).unwrap(),
            search_json(&db, "Hello", 10).unwrap(),
        ] {
            let rows: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
            let ar = rows.iter().find(|r| r["folder_id"] == fa).unwrap();
            let br = rows.iter().find(|r| r["folder_id"] == fb).unwrap();
            assert_eq!(ar["account_id"], a);
            assert_eq!(ar["delete_is_permanent"], false);
            assert_eq!(br["account_id"], b);
            assert_eq!(br["delete_is_permanent"], true);
        }
        messages::set_read_many_by_uids(&db, fa, &[7], true).unwrap();
        assert!(messages::get_by_uid(&db, fa, 7).unwrap().is_read);
        assert!(!messages::get_by_uid(&db, fb, 7).unwrap().is_read);
        assert!(undo::queue_move(&db, b, fa, &[7], MoveTarget::Trash).is_err());
        assert!(queue_delete(&db, b, fb, 7, false).is_err());
        assert!(matches!(
            queue_delete(&db, b, fb, 7, true).unwrap(),
            Queued::Permanent
        ));
        assert!(messages::is_listed(&db, fb, 7).unwrap());
        let Queued::Pending { batch, .. } = queue_delete(&db, a, fa, 7, false).unwrap() else {
            panic!("expected a reversible move");
        };
        assert!(!messages::is_listed(&db, fa, 7).unwrap());
        assert!(messages::is_listed(&db, fb, 7).unwrap());
        assert_eq!(undo::undo(&db, &batch).unwrap(), 1);
        db.conn()
            .execute("delete from folders where id = ?1", [trash])
            .unwrap();
        assert!(queue_delete(&db, a, fa, 7, false).is_err());
        assert!(messages::is_listed(&db, fa, 7).unwrap());
        assert!(matches!(
            queue_delete(&db, a, fa, 7, true).unwrap(),
            Queued::Permanent
        ));
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
