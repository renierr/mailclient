//! A local view over the subscribed inboxes of opted-in accounts.
//!
//! This creates no IMAP folder and moves no messages. Every row retains its
//! actual account, folder and UID so actions can target the original message.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::db::Db;
use crate::error::Result;
use crate::feed;
use crate::models::{Account, FolderRole};
use crate::store::{account_settings, accounts, folders, messages, settings};

/// One member inbox, with what its rows need to be shown and acted on.
struct Inbox {
    folder_id: i64,
    account_id: i64,
    account_name: String,
    account_email: String,
    path: String,
    delete_is_permanent: bool,
}

/// The member inboxes — the subscribed inbox folders of the opted-in
/// accounts, in account order. The single definition of membership: the
/// list, the counts and the search all read it.
fn inboxes(db: &Db) -> Result<Vec<Inbox>> {
    let mut out = Vec::new();
    for account in included_accounts(db)? {
        let all = folders::list_by_account(db, account.id)?;
        for folder in all
            .iter()
            .filter(|f| f.subscribed && f.role == FolderRole::Inbox)
        {
            out.push(Inbox {
                folder_id: folder.id,
                account_id: account.id,
                account_name: account.name.clone(),
                account_email: account.email_address.clone(),
                path: folder.path.clone(),
                delete_is_permanent: crate::undo::delete_is_permanent(folder, &all),
            });
        }
    }
    Ok(out)
}

/// Subscribed inbox folders for the opted-in accounts, in account order.
pub fn folder_ids(db: &Db) -> Result<Vec<i64>> {
    Ok(inboxes(db)?.into_iter().map(|i| i.folder_id).collect())
}

/// Accounts selected by the user, including those awaiting inbox discovery.
pub fn included_accounts(db: &Db) -> Result<Vec<Account>> {
    Ok(accounts::list(db)?
        .into_iter()
        .filter(|a| account_settings::combined_inbox(db, a.id))
        .collect())
}

/// Explicit foreground refresh of the opted-in accounts' inboxes,
/// continuing past an unavailable account. Returns the status line; errors
/// stay visible when other accounts succeed.
pub async fn sync(db: &Db) -> Result<String> {
    let accounts = included_accounts(db)?;
    let report = crate::sync::headless::sync_accounts(
        db,
        &accounts,
        crate::sync::headless::SyncScope::InboxOnly,
    )
    .await;
    Ok(sync_status(&report))
}

fn sync_status(report: &crate::sync::headless::SyncAllReport) -> String {
    let n = report.accounts.len();
    let base = format!(
        "Synced {n} account{}: +{} new, -{} removed",
        if n == 1 { "" } else { "s" },
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
/// `accounts` is how many opted-in accounts have a member inbox; a frontend
/// offers the view once there are two. `total` lets it page the list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub total: u64,
    pub unread: u64,
    pub accounts: u64,
}

pub fn counts(db: &Db) -> Result<Counts> {
    let mut counts = Counts::default();
    let mut account_ids = HashSet::new();
    for inbox in inboxes(db)? {
        account_ids.insert(inbox.account_id);
        counts.total += messages::count_by_folder(db, inbox.folder_id)?;
        counts.unread += messages::count_unread(db, inbox.folder_id)?;
    }
    counts.accounts = account_ids.len() as u64;
    Ok(counts)
}

/// Rows per page of the combined list. The view starts with one page and
/// grows a page at a time ("Show more"), so a large cache is never read
/// whole on every refresh.
pub const PAGE_SIZE: u64 = 200;

/// The first `pages` pages of the combined list (at least one), as
/// [`messages_json`] shapes them. A frontend has more to show while it holds
/// fewer rows than [`Counts::total`].
pub fn pages_json(db: &Db, pages: u64) -> Result<String> {
    messages_json(db, pages.max(1).saturating_mul(PAGE_SIZE), 0)
}

/// A globally sorted page of locally cached messages, attributed to accounts.
pub fn messages_json(db: &Db, limit: u64, offset: u64) -> Result<String> {
    let inboxes = inboxes(db)?;
    let ids: Vec<i64> = inboxes.iter().map(|i| i.folder_id).collect();
    let rows = messages::list_compact_by_folders_sorted(
        db,
        &ids,
        limit,
        offset,
        &settings::get_sort_field(db),
        settings::get_sort_descending(db),
    )?
    .into_iter()
    .map(|m| feed::list_row_json(m, false))
    .collect();
    decorate_rows(&inboxes, rows)
}

/// Local search hits with the same account and action metadata as list
/// rows, newest first. No server search is started.
pub fn search_json(db: &Db, query: &str, limit: u64) -> Result<String> {
    let inboxes = inboxes(db)?;
    let ids: Vec<i64> = inboxes.iter().map(|i| i.folder_id).collect();
    let rows = feed::search_hits(db, feed::SearchScope::Folders(&ids), query, limit, "")?;
    decorate_rows(&inboxes, rows)
}

fn decorate_rows(inboxes: &[Inbox], mut rows: Vec<serde_json::Value>) -> Result<String> {
    let by_folder: HashMap<i64, &Inbox> = inboxes.iter().map(|i| (i.folder_id, i)).collect();
    for row in &mut rows {
        let Some(inbox) = row["folder_id"].as_i64().and_then(|id| by_folder.get(&id)) else {
            continue;
        };
        row["folder"] = serde_json::json!(inbox.path);
        row["account_name"] = serde_json::json!(inbox.account_name);
        row["account_email"] = serde_json::json!(inbox.account_email);
        row["delete_is_permanent"] = serde_json::json!(inbox.delete_is_permanent);
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
                accounts: 2
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
                accounts: 2
            }
        );
        folders::set_subscribed(&db, fa, false).unwrap();
        assert_eq!(
            counts(&db).unwrap(),
            Counts {
                total: 0,
                unread: 0,
                accounts: 1
            }
        );
        accounts::delete(&db, b).unwrap();
        assert!(folder_ids(&db).unwrap().is_empty());
    }

    #[test]
    fn pages_grow_by_page_size() {
        let db = Db::open_in_memory().unwrap();
        let a = accounts::create_for_test(&db, "a@example.com");
        let fa = folders::upsert(&db, a, "INBOX", "/", FolderRole::Inbox).unwrap();
        opt_in(&db, a);
        for uid in 1..=(PAGE_SIZE as u32 + 5) {
            messages::upsert(&db, &messages::sample_new(a, fa, uid)).unwrap();
        }
        let rows = |pages| {
            serde_json::from_str::<Vec<serde_json::Value>>(&pages_json(&db, pages).unwrap())
                .unwrap()
                .len() as u64
        };
        assert_eq!(rows(0), PAGE_SIZE);
        assert_eq!(rows(1), PAGE_SIZE);
        assert_eq!(rows(2), PAGE_SIZE + 5);
        assert_eq!(counts(&db).unwrap().total, PAGE_SIZE + 5);
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
            serde_json::from_str(&search_json(&db, "is:unread", 20).unwrap()).unwrap();
        assert_eq!(rows.len(), 2);
        folders::set_subscribed(&db, fa, false).unwrap();
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&search_json(&db, "Hello", 20).unwrap()).unwrap();
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
        assert_eq!(counts(&db).unwrap().accounts, 1);
        assert_eq!(included_accounts(&db).unwrap().len(), 1);
        let report = crate::sync::headless::SyncAllReport {
            accounts: vec![crate::sync::headless::AccountSyncResult::default()],
            total_fetched: 3,
            errors: vec!["offline".into()],
            ..Default::default()
        };
        assert_eq!(
            sync_status(&report),
            "Synced 1 account: +3 new, -0 removed; 1 error(s): offline"
        );
    }
}
