//! Folder-operation semantics: `Seen` enforcement on trash moves, pushing
//! queued (undoable) moves, and attachment fetching.

use crate::db::Db;
use crate::models::FolderRole;
use crate::store::{accounts, folders, messages};
use crate::sync::imap::{
    mock::{test_mock_account, MockImapServer},
    ImapSync,
};

#[tokio::test]
async fn test_mock_move_uids_to_trash_marks_seen() {
    let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            vec![
                "* 2 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 100] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else if upper.starts_with("UID STORE") {
            vec![format!("{tag} OK STORE completed\r\n")]
        } else if upper.starts_with("UID MOVE") {
            vec![format!("{tag} OK UID MOVE completed\r\n")]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = accounts::create(
        &db,
        &crate::models::NewAccount {
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
    .unwrap();

    let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
    let _trash_id = folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();

    sync.move_uids_to(&db, inbox_id, &[55, 56], "Trash")
        .await
        .unwrap();

    let cmds = server.received.lock().await.clone();
    let store_idx = cmds
        .iter()
        .position(|c| c.contains("STORE") && c.contains("\\Seen"));
    let move_idx = cmds.iter().position(|c| c.contains("UID MOVE"));

    assert!(
        store_idx.is_some(),
        "UID STORE \\Seen was not called when moving to Trash"
    );
    assert!(move_idx.is_some(), "UID MOVE was not called");
    assert!(
        store_idx.unwrap() < move_idx.unwrap(),
        "UID STORE \\Seen must occur BEFORE UID MOVE"
    );
}

#[tokio::test]
async fn fetch_attachments_missing_uid_errors_and_flag_writer_works() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            return vec![
                "* 1 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 2] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ];
        }
        // UID FETCH answers OK with no FETCH data: UID is gone server-side.
        vec![format!("{tag} OK UID FETCH completed\r\n")]
    })
    .await;
    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = accounts::create(
        &db,
        &crate::models::NewAccount {
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
    .unwrap();
    let inbox_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
    let mut m = messages::sample_new(account_id, inbox_id, 11);
    m.has_attachments = false;
    let msg_id = messages::upsert(&db, &m).unwrap();
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    // Gone server-side → InvalidInput "no longer on server" (not silent Ok(0)).
    let err = sync.fetch_attachments(&db, msg_id).await.unwrap_err();
    assert!(
        err.to_string().contains("no longer on server"),
        "got: {err}"
    );
    // And the flag writer the fetch path relies on works.
    messages::set_has_attachments(&db, msg_id, true).unwrap();
    assert!(messages::get(&db, msg_id).unwrap().has_attachments);
}

#[tokio::test]
async fn test_mock_push_due_moves_moves_once_per_group_and_skips_undue() {
    use crate::store::pending_moves::{self, PendingAction};
    let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            vec![
                "* 3 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 100] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = accounts::create(
        &db,
        &crate::models::NewAccount {
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
    .unwrap();
    let inbox = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
    folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();
    let a = messages::upsert(&db, &messages::sample_new(account_id, inbox, 11)).unwrap();
    let b = messages::upsert(&db, &messages::sample_new(account_id, inbox, 12)).unwrap();
    let c = messages::upsert(&db, &messages::sample_new(account_id, inbox, 13)).unwrap();
    let past = "2000-01-01T00:00:00Z";
    pending_moves::queue(&db, &[a, b], PendingAction::Trash, None, "due", past).unwrap();
    pending_moves::queue(
        &db,
        &[c],
        PendingAction::Trash,
        None,
        "later",
        "2999-01-01T00:00:00Z",
    )
    .unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    assert_eq!(sync.push_due_moves(&db, account_id).await, 2);

    let cmds = server.received.lock().await.clone();
    let moves: Vec<&String> = cmds.iter().filter(|c| c.contains("UID MOVE")).collect();
    assert_eq!(
        moves.len(),
        1,
        "one round trip for the whole batch: {cmds:?}"
    );
    assert!(moves[0].contains("11") && moves[0].contains("12") && !moves[0].contains("13"));
    assert!(moves[0].contains("Trash"));
    assert!(cmds
        .iter()
        .any(|c| c.contains("STORE") && c.contains("\\Seen")));
    // Moved rows are gone locally; the undue one is still queued and hidden.
    assert!(messages::get(&db, a).is_err());
    assert!(messages::get(&db, c).is_ok());
    assert!(pending_moves::any_for_account(&db, account_id).unwrap());
}
