//! Engine sync-flow tests: windowed sync (fetch + flag refresh + expunge),
//! older-mail backfill, server search backfill, UIDVALIDITY resync, and the
//! Trash `Seen` sweep — all against the mock server with a real SQLite DB.

use crate::db::Db;
use crate::models::{FolderRole, NewAccount};
use crate::store::{accounts, folders, messages};
use crate::sync::imap::{
    mock::{test_mock_account, MockImapServer},
    ImapSync, FULL_SYNC_WINDOW,
};

fn test_account_row(db: &Db, account: &crate::models::Account) -> i64 {
    accounts::create(
        db,
        &NewAccount {
            name: account.name.clone(),
            email_address: account.email_address.clone(),
            from_name: account.from_name.clone(),
            imap_host: account.imap_host.clone(),
            imap_port: account.imap_port,
            imap_security: account.imap_security.clone(),
            imap_username: account.imap_username.clone(),
            smtp_host: account.smtp_host.clone(),
            smtp_port: account.smtp_port,
            smtp_security: account.smtp_security.clone(),
            smtp_username: account.smtp_username.clone(),
            auth_vault_key: account.auth_vault_key.clone(),
            check_interval_secs: account.check_interval_secs,
        },
    )
    .unwrap()
}

fn select_ok(tag: &str, exists: u32, validity: u32, next: u32) -> Vec<String> {
    vec![
        format!("* {exists} EXISTS\r\n"),
        format!("* OK [UIDVALIDITY {validity}] Ok\r\n"),
        format!("* OK [UIDNEXT {next}] Ok\r\n"),
        format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
    ]
}

fn fetch_literal(uid: u32, seq: u32, raw: &str) -> String {
    format!(
        "* {seq} FETCH (UID {uid} FLAGS () BODY[] {{{}}}\r\n{raw})\r\n",
        raw.len()
    )
}

const RAW6: &str = "From: a@x.y\r\nSubject: six\r\nContent-Type: text/plain\r\n\r\nbody six";
const RAW7: &str = "From: a@x.y\r\nSubject: seven\r\nContent-Type: text/plain\r\n\r\nbody seven";

#[tokio::test]
async fn sync_window_fetches_refreshes_and_expunges() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            select_ok(tag, 3, 1, 8)
        } else if upper.starts_with("UID SEARCH") {
            vec![
                "* SEARCH 5 6 7\r\n".to_string(),
                format!("{tag} OK UID SEARCH completed\r\n"),
            ]
        } else if upper.starts_with("UID FETCH") && upper.contains("BODY") {
            vec![
                fetch_literal(6, 2, RAW6),
                fetch_literal(7, 3, RAW7),
                format!("{tag} OK UID FETCH completed\r\n"),
            ]
        } else if upper.starts_with("UID FETCH") {
            vec![
                "* 1 FETCH (UID 5 FLAGS (\\Seen))\r\n".to_string(),
                format!("{tag} OK UID FETCH completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = test_account_row(&db, &account);
    let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();

    // Local uid 5 is unread (flag refresh must mark it read); uid 4 is gone
    // server-side (below search_lo, must be expunged).
    let mut m5 = messages::sample_new(account_id, inbox_id, 5);
    m5.is_read = false;
    messages::upsert(&db, &m5).unwrap();
    messages::upsert(&db, &messages::sample_new(account_id, inbox_id, 4)).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let report = sync
        .sync_folder_window(&db, inbox_id, Some(FULL_SYNC_WINDOW))
        .await
        .unwrap();

    assert_eq!(report.fetched, 2);
    assert_eq!(report.expunged, 1);
    let uids = messages::list_uids(&db, inbox_id).unwrap();
    assert!(uids.contains(&5) && uids.contains(&6) && uids.contains(&7));
    assert!(!uids.contains(&4), "stale uid must be expunged");
    let m5 = messages::get_by_uid(&db, inbox_id, 5).unwrap();
    assert!(m5.is_read, "flag refresh must mark uid 5 read");
    assert_eq!(
        messages::get_by_uid(&db, inbox_id, 6)
            .unwrap()
            .subject
            .as_deref(),
        Some("six")
    );
    let folder = folders::get(&db, inbox_id).unwrap();
    assert_eq!(folder.uid_validity, Some(1));
    assert_eq!(folder.uid_next, Some(8));
}

#[tokio::test]
async fn uidvalidity_change_drops_local_cache() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            select_ok(tag, 0, 2, 6)
        } else if upper.starts_with("UID SEARCH") {
            vec![
                "* SEARCH\r\n".to_string(),
                format!("{tag} OK UID SEARCH completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = test_account_row(&db, &account);
    let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
    folders::set_sync_state(&db, inbox_id, 1, 10, 1, 0).unwrap();
    messages::upsert(&db, &messages::sample_new(account_id, inbox_id, 5)).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    sync.sync_folder_window(&db, inbox_id, Some(FULL_SYNC_WINDOW))
        .await
        .unwrap();

    assert!(messages::list_uids(&db, inbox_id).unwrap().is_empty());
    assert_eq!(folders::get(&db, inbox_id).unwrap().uid_validity, Some(2));
}

#[tokio::test]
async fn sync_older_backfills_below_local_min() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            select_ok(tag, 5, 1, 11)
        } else if upper.contains("1:5") {
            vec![
                "* SEARCH 3 4 5\r\n".to_string(),
                format!("{tag} OK UID SEARCH completed\r\n"),
            ]
        } else if upper.starts_with("UID FETCH") {
            vec![
                fetch_literal(3, 1, RAW6),
                fetch_literal(4, 2, RAW7),
                format!("{tag} OK UID FETCH completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = test_account_row(&db, &account);
    let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
    for uid in [6u32, 7, 8] {
        messages::upsert(&db, &messages::sample_new(account_id, inbox_id, uid)).unwrap();
    }

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let report = sync.sync_older(&db, inbox_id, 10).await.unwrap();
    assert_eq!(report.fetched, 2);
    let uids = messages::list_uids(&db, inbox_id).unwrap();
    assert!(uids.contains(&3) && uids.contains(&4));
}

#[tokio::test]
async fn server_search_backfills_missing_uids() {
    const RAW9: &str =
        "From: a@x.y\r\nSubject: hello world\r\nContent-Type: text/plain\r\n\r\nhello";
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            select_ok(tag, 1, 1, 10)
        } else if upper.contains("TEXT") {
            vec![
                "* SEARCH 9\r\n".to_string(),
                format!("{tag} OK UID SEARCH completed\r\n"),
            ]
        } else if upper.starts_with("UID FETCH") {
            vec![
                fetch_literal(9, 1, RAW9),
                format!("{tag} OK UID FETCH completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = test_account_row(&db, &account);
    folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let report = sync
        .search_server_into_cache(&db, account_id, &["hello".to_string()], None)
        .await
        .unwrap();
    assert_eq!(report.folders_searched, 1);
    assert_eq!(report.fetched, 1);
    let folder = folders::get_by_path(&db, account_id, "INBOX").unwrap();
    assert_eq!(
        messages::get_by_uid(&db, folder.id, 9)
            .unwrap()
            .subject
            .as_deref(),
        Some("hello world")
    );
}

#[tokio::test]
async fn trash_sweep_marks_unread_seen_on_server() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            select_ok(tag, 1, 1, 6)
        } else if upper.starts_with("UID SEARCH") {
            vec![
                "* SEARCH 5 6\r\n".to_string(),
                format!("{tag} OK UID SEARCH completed\r\n"),
            ]
        } else if upper.starts_with("UID FETCH") {
            vec![
                "* 1 FETCH (UID 5 FLAGS ())\r\n".to_string(),
                "* 2 FETCH (UID 6 FLAGS ())\r\n".to_string(),
                format!("{tag} OK UID FETCH completed\r\n"),
            ]
        } else if upper.starts_with("UID STORE") {
            vec![format!("{tag} OK STORE completed\r\n")]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = test_account_row(&db, &account);
    let trash_id = folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();
    let mut m = messages::sample_new(account_id, trash_id, 5);
    m.is_read = false;
    messages::upsert(&db, &m).unwrap();
    // User-marked-unread in Trash: locally dirty, so the flag refresh skips
    // it (flags_dirty) and only the sweep can push Seen server-side.
    messages::set_read_many_by_uids(&db, trash_id, &[5], false).unwrap();
    // Clean unread row: the flag refresh marks it read (Trash is always read).
    let mut m6 = messages::sample_new(account_id, trash_id, 6);
    m6.is_read = false;
    messages::upsert(&db, &m6).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    sync.sync_folder_window(&db, trash_id, Some(FULL_SYNC_WINDOW))
        .await
        .unwrap();

    let cmds = server.received.lock().await.clone();
    assert!(
        cmds.iter()
            .any(|c| c.contains("STORE") && c.contains("\\Seen")),
        "trash sweep must STORE \\Seen: {cmds:?}"
    );
    // Clean row converged via the flag refresh …
    assert!(messages::get_by_uid(&db, trash_id, 6).unwrap().is_read);
    // … while the user-dirtied row keeps its local unread mark (flags_dirty
    // is respected) even though the server copy is now Seen.
    assert!(!messages::get_by_uid(&db, trash_id, 5).unwrap().is_read);
}

#[tokio::test]
async fn push_dirty_flags_clears_on_success() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            select_ok(tag, 1, 1, 6)
        } else if upper.starts_with("UID STORE") {
            vec![format!("{tag} OK STORE completed\r\n")]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = test_account_row(&db, &account);
    let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
    let mut m = messages::sample_new(account_id, inbox_id, 5);
    m.is_read = true;
    m.is_starred = true;
    let id = messages::upsert(&db, &m).unwrap();
    messages::set_flags(&db, id, true, true).unwrap();
    assert_eq!(
        messages::list_flags_dirty(&db, account_id).unwrap().len(),
        1
    );

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    assert_eq!(sync.push_dirty_flags(&db, account_id).await, 1);
    assert!(messages::list_flags_dirty(&db, account_id)
        .unwrap()
        .is_empty());

    let cmds = server.received.lock().await.clone();
    assert!(cmds
        .iter()
        .any(|c| c.contains("STORE") && c.contains("\\Seen")));
    assert!(cmds
        .iter()
        .any(|c| c.contains("STORE") && c.contains("\\Flagged")));
}

#[tokio::test]
async fn push_dirty_flags_keeps_rows_on_failure() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            select_ok(tag, 1, 1, 6)
        } else if upper.starts_with("UID STORE") {
            vec![format!("{tag} NO STORE failed\r\n")]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = test_account_row(&db, &account);
    let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
    let id = messages::upsert(&db, &messages::sample_new(account_id, inbox_id, 5)).unwrap();
    messages::set_flags(&db, id, true, false).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    assert_eq!(sync.push_dirty_flags(&db, account_id).await, 0);
    assert_eq!(
        messages::list_flags_dirty(&db, account_id).unwrap().len(),
        1
    );
}
