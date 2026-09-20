//! Protocol-level guards: capability gating, SELECT / MOVE / CHANGEDSINCE
//! fallbacks, `BYE` handling, greeting refusal, and NOOP health probing.

use crate::sync::imap::{
    mock::{test_mock_account, MockImapServer},
    ImapSync,
};
use imap_types::flag::Flag;

#[tokio::test]
async fn test_mock_capabilities_guard_no_extensions() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            vec![
                "* 10 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 100] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else if upper.starts_with("UID COPY") {
            vec![format!("{tag} OK UID COPY completed\r\n")]
        } else if upper.starts_with("UID STORE") {
            vec![format!("{tag} OK STORE completed\r\n")]
        } else if upper.starts_with("EXPUNGE") {
            vec![
                "* 1 EXPUNGE\r\n".to_string(),
                format!("{tag} OK EXPUNGE completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();

    {
        let session = sync.session().unwrap();
        assert!(session.has_capability("IMAP4rev1"));
        assert!(!session.condstore_enabled);
        assert!(!session.qresync_enabled);
        assert!(!session.has_capability("MOVE"));
        assert!(!session.has_capability("ENABLE"));
    }

    let session = sync.session.as_mut().unwrap();
    session.select("INBOX", None).await.unwrap();
    session.uid_move(&[10], "Trash").await.unwrap();

    let cmds = server.received.lock().await.clone();
    // Assert ENABLE was never sent
    assert!(!cmds
        .iter()
        .any(|c| c.to_ascii_uppercase().contains("ENABLE")));
    // Assert SELECT was sent as standard SELECT without CONDSTORE or QRESYNC
    assert!(cmds
        .iter()
        .any(|c| c.contains("SELECT") && !c.contains("CONDSTORE") && !c.contains("QRESYNC")));
    // Assert UID MOVE was NEVER sent, but COPY + STORE \Deleted + EXPUNGE was used
    assert!(!cmds
        .iter()
        .any(|c| c.to_ascii_uppercase().contains("UID MOVE")));
    assert!(cmds
        .iter()
        .any(|c| c.to_ascii_uppercase().contains("UID COPY 10")));
    assert!(cmds
        .iter()
        .any(|c| c.contains("STORE") && c.contains("\\Deleted")));
    assert!(cmds
        .iter()
        .any(|c| c.to_ascii_uppercase().contains("EXPUNGE")));
}

#[tokio::test]
async fn test_mock_select_fallback_on_unsupported_condstore() {
    let server = MockImapServer::start("IMAP4rev1 CONDSTORE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.contains("(CONDSTORE)") {
            // Server rejects the extension parameter
            vec![format!("{tag} BAD [CANNOT] parameter not supported\r\n")]
        } else if upper.starts_with("SELECT") {
            vec![
                "* 5 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 50] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();

    let session = sync.session.as_mut().unwrap();
    assert!(session.condstore_enabled);

    let sel = session.select("INBOX", None).await.unwrap();
    assert_eq!(sel.exists, 5);
    // condstore should now be disabled due to the fallback
    assert!(!session.condstore_enabled);

    let cmds = server.received.lock().await.clone();
    // First SELECT attempted with CONDSTORE
    assert!(cmds
        .iter()
        .any(|c| c.contains("SELECT") && c.contains("CONDSTORE")));
    // Second SELECT fell back to standard SELECT
    assert!(cmds
        .iter()
        .any(|c| c.contains("SELECT") && !c.contains("CONDSTORE")));
}

#[tokio::test]
async fn test_mock_uid_move_fallback_when_server_rejects_move() {
    let server = MockImapServer::start("IMAP4rev1 MOVE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("UID MOVE") {
            // Server advertised MOVE but failed the command
            vec![format!(
                "{tag} NO [CANNOT] UID MOVE not supported on this folder\r\n"
            )]
        } else if upper.starts_with("UID COPY") {
            vec![format!("{tag} OK UID COPY completed\r\n")]
        } else if upper.starts_with("UID STORE") {
            vec![format!("{tag} OK STORE completed\r\n")]
        } else if upper.starts_with("EXPUNGE") {
            vec![
                "* 1 EXPUNGE\r\n".to_string(),
                format!("{tag} OK EXPUNGE completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();

    let session = sync.session.as_mut().unwrap();
    assert!(session.has_capability("MOVE"));

    session.uid_move(&[42], "Trash").await.unwrap();

    let cmds = server.received.lock().await.clone();
    // UID MOVE was tried first
    assert!(cmds
        .iter()
        .any(|c| c.to_ascii_uppercase().contains("UID MOVE 42")));
    // Fallback sequence was executed
    assert!(cmds
        .iter()
        .any(|c| c.to_ascii_uppercase().contains("UID COPY 42")));
    assert!(cmds
        .iter()
        .any(|c| c.contains("STORE") && c.contains("\\Deleted")));
    assert!(cmds
        .iter()
        .any(|c| c.to_ascii_uppercase().contains("EXPUNGE")));
}

#[tokio::test]
async fn test_mock_changesince_fallback_when_server_rejects_modifier() {
    let server = MockImapServer::start("IMAP4rev1 CONDSTORE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.contains("CHANGEDSINCE") {
            vec![format!("{tag} BAD Unknown modifier CHANGEDSINCE\r\n")]
        } else if upper.starts_with("UID FETCH") {
            vec![
                "* 1 FETCH (UID 7 FLAGS (\\Seen))\r\n".to_string(),
                format!("{tag} OK UID FETCH completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;

    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();

    let session = sync.session.as_mut().unwrap();
    assert!(session.condstore_enabled);

    let res = session
        .uid_fetch_flags_changesince(&[7], 100)
        .await
        .unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].0, 7);
    assert_eq!(res[0].1, vec![Flag::Seen]);
    // condstore should now be disabled due to fallback
    assert!(!session.condstore_enabled);

    let cmds = server.received.lock().await.clone();
    assert!(cmds.iter().any(|c| c.contains("CHANGEDSINCE")));
    assert!(cmds
        .iter()
        .any(|c| c.contains("UID FETCH") && !c.contains("CHANGEDSINCE")));
}

#[tokio::test]
async fn bye_during_command_is_an_error_not_a_hang() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        if rest.to_ascii_uppercase().starts_with("NOOP") {
            vec![
                "* BYE server is shutting down\r\n".to_string(),
                format!("{tag} OK NOOP completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let err = sync.session().unwrap().noop().await.unwrap_err();
    assert!(
        err.to_string().contains("BYE"),
        "expected BYE error, got: {err}"
    );
}

#[tokio::test]
async fn bye_greeting_fails_connect_fast() {
    let server = MockImapServer::start_with_greeting(
        "IMAP4rev1",
        "* BYE server is down\r\n".to_string(),
        |tag, _| vec![format!("{tag} OK completed\r\n")],
    )
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    let err = sync.connect("secret").await.unwrap_err();
    assert!(
        err.to_string().contains("BYE"),
        "expected BYE greeting error, got: {err}"
    );
}

#[tokio::test]
async fn healthy_session_passes_noop_probe() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        if rest.to_ascii_uppercase().starts_with("NOOP") {
            vec![format!("{tag} OK NOOP completed\r\n")]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    assert!(!sync.is_healthy().await);
    sync.connect("secret").await.unwrap();
    assert!(sync.is_healthy().await);
    sync.logout().await;
    assert!(!sync.is_healthy().await);
}
