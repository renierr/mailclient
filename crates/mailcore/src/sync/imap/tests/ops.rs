//! Folder-operation semantics: `Seen` enforcement on trash moves, the Junk
//! shortcut, the cross-account guard, and attachment fetching.

use crate::db::Db;
use crate::models::FolderRole;
use crate::store::{accounts, folders, messages};
use crate::sync::imap::{
    mock::{test_mock_account, MockImapServer},
    ImapSync, MoveOutcome, TrashOutcome,
};

#[tokio::test]
async fn test_mock_trash_message_marks_seen_before_move() {
    let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            vec![
                "* 1 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 100] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else if upper.starts_with("UID STORE") {
            vec![
                "* 1 FETCH (UID 99 FLAGS (\\Seen))\r\n".to_string(),
                format!("{tag} OK STORE completed\r\n"),
            ]
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

    // Create an unread message
    let mut new_msg = messages::sample_new(account_id, inbox_id, 99);
    new_msg.is_read = false;
    let msg_id = messages::upsert(&db, &new_msg).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();

    let outcome = sync.trash_message(&db, msg_id).await.unwrap();
    assert_eq!(outcome, TrashOutcome::Moved("Trash".to_string()));

    let cmds = server.received.lock().await.clone();
    let store_idx = cmds
        .iter()
        .position(|c| c.contains("STORE") && c.contains("\\Seen"));
    let move_idx = cmds.iter().position(|c| c.contains("UID MOVE"));

    assert!(
        store_idx.is_some(),
        "UID STORE \\Seen was not called for unread message"
    );
    assert!(move_idx.is_some(), "UID MOVE was not called");
    assert!(
        store_idx.unwrap() < move_idx.unwrap(),
        "UID STORE \\Seen must occur BEFORE UID MOVE"
    );
}

#[tokio::test]
async fn test_mock_trash_message_always_marks_seen() {
    let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            vec![
                "* 1 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 100] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else if upper.starts_with("UID STORE") {
            vec![format!("{tag} OK UID STORE completed\r\n")]
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

    // Create an ALREADY-READ message
    let mut new_msg = messages::sample_new(account_id, inbox_id, 99);
    new_msg.is_read = true;
    let msg_id = messages::upsert(&db, &new_msg).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();

    let outcome = sync.trash_message(&db, msg_id).await.unwrap();
    assert_eq!(outcome, TrashOutcome::Moved("Trash".to_string()));

    let cmds = server.received.lock().await.clone();
    let store_idx = cmds
        .iter()
        .position(|c| c.contains("STORE") && c.contains("\\Seen"));
    let move_idx = cmds.iter().position(|c| c.contains("UID MOVE"));

    assert!(
        store_idx.is_some(),
        "UID STORE \\Seen must be called when trashing to guarantee message is seen"
    );
    assert!(move_idx.is_some(), "UID MOVE was not called");
    assert!(
        store_idx.unwrap() < move_idx.unwrap(),
        "UID STORE \\Seen must occur BEFORE UID MOVE"
    );
}

#[tokio::test]
async fn test_mock_move_to_folder_trash_marks_seen() {
    let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            vec![
                "* 1 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 100] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else if upper.starts_with("UID STORE") {
            vec![format!("{tag} OK UID STORE completed\r\n")]
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
    let trash_id = folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();

    let new_msg = messages::sample_new(account_id, inbox_id, 88);
    let msg_id = messages::upsert(&db, &new_msg).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();

    let outcome = sync.move_to_folder(&db, msg_id, trash_id).await.unwrap();
    assert_eq!(outcome, MoveOutcome::Moved("Trash".to_string()));

    let cmds = server.received.lock().await.clone();
    let store_idx = cmds
        .iter()
        .position(|c| c.contains("STORE") && c.contains("\\Seen"));
    let move_idx = cmds.iter().position(|c| c.contains("UID MOVE"));

    assert!(
        store_idx.is_some(),
        "UID STORE \\Seen must be called in move_to_folder when target is Trash"
    );
    assert!(move_idx.is_some(), "UID MOVE was not called");
    assert!(
        store_idx.unwrap() < move_idx.unwrap(),
        "UID STORE \\Seen must occur BEFORE UID MOVE"
    );
}

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
async fn trash_destroys_junk_directly() {
    let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            return vec![
                "* 1 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 100] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ];
        } else if upper.starts_with("UID STORE") {
            return vec![format!("{tag} OK STORE completed\r\n")];
        } else if upper.starts_with("EXPUNGE") {
            return vec![
                "* 1 EXPUNGE\r\n".to_string(),
                format!("{tag} OK EXPUNGE completed\r\n"),
            ];
        }
        vec![format!("{tag} OK completed\r\n")]
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
    let junk_id = folders::upsert(&db, account_id, "Junk", "/", FolderRole::Junk).unwrap();
    let _trash_id = folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();
    let msg_id = messages::upsert(&db, &messages::sample_new(account_id, junk_id, 7)).unwrap();
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let outcome = sync.trash_message(&db, msg_id).await.unwrap();
    assert_eq!(outcome, TrashOutcome::Expunged);
    let cmds = server.received.lock().await.clone();
    assert!(
        !cmds
            .iter()
            .any(|c| c.to_ascii_uppercase().contains("UID MOVE")
                || c.to_ascii_uppercase().contains("UID COPY")),
        "junk must not be moved/copied to Trash: {cmds:?}"
    );
    assert!(
        cmds.iter().any(|c| c.contains("\\Deleted")),
        "junk destroy must STORE \\Deleted: {cmds:?}"
    );
}

#[tokio::test]
async fn move_to_folder_rejects_cross_account() {
    let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, _| {
        vec![format!("{tag} OK completed\r\n")]
    })
    .await;
    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let mk = |email: &str| {
        accounts::create(
            &db,
            &crate::models::NewAccount {
                name: "t".to_string(),
                email_address: email.to_string(),
                from_name: String::new(),
                imap_host: account.imap_host.clone(),
                imap_port: account.imap_port,
                imap_security: account.imap_security.clone(),
                imap_username: account.imap_username.clone(),
                smtp_host: account.smtp_host.clone(),
                smtp_port: account.smtp_port,
                smtp_security: account.smtp_security.clone(),
                smtp_username: account.smtp_username.clone(),
                auth_vault_key: format!("k-{email}"),
                check_interval_secs: 60,
            },
        )
        .unwrap()
    };
    let acc_a = mk("a@x.y");
    let acc_b = mk("b@x.y");
    let inbox_a = folders::upsert(&db, acc_a, "INBOX", "/", FolderRole::Inbox).unwrap();
    let inbox_b = folders::upsert(&db, acc_b, "INBOX", "/", FolderRole::Inbox).unwrap();
    let msg_id = messages::upsert(&db, &messages::sample_new(acc_a, inbox_a, 9)).unwrap();
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let err = sync.move_to_folder(&db, msg_id, inbox_b).await.unwrap_err();
    assert!(err.to_string().contains("another account"), "got: {err}");
    let cmds = server.received.lock().await.clone();
    assert!(
        !cmds
            .iter()
            .any(|c| c.to_ascii_uppercase().contains("UID MOVE")),
        "no server move may happen cross-account: {cmds:?}"
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
