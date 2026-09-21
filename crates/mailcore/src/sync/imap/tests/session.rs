//! Protocol-verb tests: LIST/LSUB parsing, CAPABILITY collection, SELECT
//! results (incl. QRESYNC VANISHED ranges), UID SEARCH extraction, FETCH
//! with server-side literals, STORE/COPY/EXPUNGE/CREATE/APPEND wiring, and
//! NAMESPACE prefixes — all against the mock server, no network.

use imap_types::flag::{Flag, StoreType};

use crate::sync::imap::{
    is_selectable,
    mock::{test_mock_account, MockImapServer},
    ImapSync,
};

#[tokio::test]
async fn list_parses_names_delimiters_and_attributes() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        if rest.to_ascii_uppercase().starts_with("LIST") {
            vec![
                "* LIST (\\HasNoChildren) \"/\" INBOX\r\n".to_string(),
                "* LIST (\\HasNoChildren) \".\" INBOX.Archive\r\n".to_string(),
                "* LIST (\\Noselect) \"/\" Ghosts\r\n".to_string(),
                format!("{tag} OK LIST completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    let found = session.list("", "*").await.unwrap();
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].name, "INBOX");
    assert_eq!(found[0].delimiter, "/");
    assert_eq!(found[1].name, "INBOX.Archive");
    assert_eq!(found[1].delimiter, ".");
    assert!(is_selectable(&found[0].attributes));
    assert!(!is_selectable(&found[2].attributes));
}

#[tokio::test]
async fn lsub_returns_subscribed_and_capability_dedups() {
    let server = MockImapServer::start("IMAP4rev1 IMAP4rev1 IDLE IDLE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("LSUB") {
            vec![
                "* LSUB (\\HasNoChildren) \"/\" INBOX\r\n".to_string(),
                format!("{tag} OK LSUB completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    let subs = session.lsub("", "*").await.unwrap();
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].name, "INBOX");
    let caps = session.capability().await.unwrap();
    // The codec uppercases capability atoms; collection still sorts + dedups.
    assert_eq!(caps, vec!["IDLE".to_string(), "IMAP4REV1".to_string()]);
}

#[tokio::test]
async fn enable_advertised_extensions_are_recorded() {
    let server = MockImapServer::start("IMAP4rev1 ENABLE CONDSTORE QRESYNC", |tag, _| {
        vec![format!("{tag} OK completed\r\n")]
    })
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    assert!(session.condstore_enabled);
    assert!(session.qresync_enabled);
}

#[tokio::test]
async fn select_reads_counts_validity_next_modseq_and_vanished() {
    let server = MockImapServer::start("IMAP4rev1 CONDSTORE QRESYNC", |tag, rest| {
        if rest.to_ascii_uppercase().starts_with("SELECT") {
            vec![
                "* 3 EXISTS\r\n".to_string(),
                "* VANISHED 1:2\r\n".to_string(),
                "* OK [UIDVALIDITY 42] Ok\r\n".to_string(),
                "* OK [UIDNEXT 100] Ok\r\n".to_string(),
                "* OK [HIGHESTMODSEQ 777] Highest\r\n".to_string(),
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
    let session = sync.session().unwrap();
    let sel = session.select("INBOX", None).await.unwrap();
    assert_eq!(sel.exists, 3);
    assert_eq!(sel.uid_validity, Some(42));
    assert_eq!(sel.uid_next, Some(100));
    assert_eq!(sel.highest_modseq, Some(777));
    assert_eq!(sel.vanished, vec![(1, 2)]);
}

#[tokio::test]
async fn uid_search_extracts_uids() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        if rest.to_ascii_uppercase().starts_with("UID SEARCH") {
            vec![
                "* SEARCH 5 9 12\r\n".to_string(),
                format!("{tag} OK UID SEARCH completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    let uids = session
        .uid_search(crate::sync::imap::vec1!(imap_types::search::SearchKey::All))
        .await
        .unwrap();
    assert_eq!(uids, vec![5, 9, 12]);
}

#[tokio::test]
async fn uid_fetch_messages_parses_server_literal() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            vec![
                "* 1 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 2] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else if upper.starts_with("UID FETCH") {
            vec![
                "* 1 FETCH (UID 5 FLAGS (\\Seen) BODY[] {11}\r\nhello world)\r\n".to_string(),
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
    let session = sync.session().unwrap();
    session.select("INBOX", None).await.unwrap();
    let fetched = session.uid_fetch_messages(&[5]).await.unwrap();
    assert_eq!(fetched.len(), 1);
    assert_eq!(fetched[0].0, 5);
    assert_eq!(fetched[0].1, vec![Flag::Seen]);
    assert_eq!(fetched[0].2, b"hello world");
}

/// Mock that answers SELECT and accepts everything else, recording each
/// command so a test can assert which expunge verb actually went out.
async fn expunge_server(caps: &'static str) -> MockImapServer {
    MockImapServer::start(caps, |tag, rest| {
        if rest.to_ascii_uppercase().starts_with("SELECT") {
            vec![
                "* 0 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await
}

/// `true` when the command's verb (after the tag) is a bare `EXPUNGE`, i.e.
/// the mailbox-wide form rather than `UID EXPUNGE`.
fn is_bare_expunge(cmd: &str) -> bool {
    cmd.to_ascii_uppercase().split_whitespace().nth(1) == Some("EXPUNGE")
}

#[tokio::test]
async fn uid_expunge_scopes_the_destroy_to_the_given_uids() {
    // With UIDPLUS the destroy must name its UIDs: a bare EXPUNGE would also
    // destroy whatever another client had flagged \Deleted but not expunged.
    let server = expunge_server("IMAP4rev1 UIDPLUS").await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    session.select("INBOX", None).await.unwrap();
    session.uid_expunge(&[5, 6]).await.unwrap();

    let cmds = server.received.lock().await.clone();
    assert!(
        cmds.iter()
            .any(|c| c.to_ascii_uppercase().contains("UID EXPUNGE 5:6")),
        "expected a scoped UID EXPUNGE, got: {cmds:?}"
    );
    assert!(
        !cmds.iter().any(|c| is_bare_expunge(c)),
        "a mailbox-wide EXPUNGE slipped out: {cmds:?}"
    );
}

#[tokio::test]
async fn uid_expunge_falls_back_without_uidplus() {
    // No scoped form exists there, and leaving the messages flagged-but-alive
    // would be the wrong outcome too.
    let server = expunge_server("IMAP4rev1").await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    session.select("INBOX", None).await.unwrap();
    session.uid_expunge(&[5]).await.unwrap();

    let cmds = server.received.lock().await.clone();
    assert!(
        cmds.iter().any(|c| is_bare_expunge(c)),
        "expected the plain EXPUNGE fallback, got: {cmds:?}"
    );
}

#[tokio::test]
async fn uid_expunge_of_nothing_stays_off_the_wire() {
    let server = expunge_server("IMAP4rev1 UIDPLUS").await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    session.select("INBOX", None).await.unwrap();
    session.uid_expunge(&[]).await.unwrap();

    let cmds = server.received.lock().await.clone();
    assert!(
        !cmds
            .iter()
            .any(|c| c.to_ascii_uppercase().contains("EXPUNGE")),
        "an empty selection still sent an expunge: {cmds:?}"
    );
}

#[tokio::test]
async fn store_copy_expunge_and_create_hit_the_wire() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("SELECT") {
            vec![
                "* 1 EXISTS\r\n".to_string(),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                "* OK [UIDNEXT 2] Ok\r\n".to_string(),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]
        } else if upper.starts_with("UID STORE") {
            vec![
                "* 1 FETCH (UID 5 FLAGS (\\Seen))\r\n".to_string(),
                format!("{tag} OK STORE completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    session.select("INBOX", None).await.unwrap();
    session
        .uid_store_flags(&[5], StoreType::Add, vec![Flag::Seen])
        .await
        .unwrap();
    session.uid_copy(&[5], "Archive").await.unwrap();
    session.expunge().await.unwrap();
    session.create_folder("Archive").await.unwrap();
    session.noop().await.unwrap();
    let cmds = server.received.lock().await.clone();
    for verb in ["UID STORE 5", "UID COPY 5", "EXPUNGE", "CREATE", "NOOP"] {
        assert!(
            cmds.iter().any(|c| c.to_ascii_uppercase().contains(verb)),
            "{verb} missing from: {cmds:?}"
        );
    }
}

#[tokio::test]
async fn append_sends_literal_after_continuation() {
    use std::sync::{Arc, Mutex};
    let in_flight: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let sending = Arc::clone(&in_flight);
    let server = MockImapServer::start("IMAP4rev1", move |tag, rest| {
        if rest.to_ascii_uppercase().starts_with("APPEND") {
            *sending.lock().unwrap() = Some(tag.to_string());
            vec!["+ Ready for literal\r\n".to_string()]
        } else {
            match sending.lock().unwrap().take() {
                Some(t) => vec![format!("{t} OK APPEND completed\r\n")],
                None => vec![format!("{tag} OK completed\r\n")],
            }
        }
    })
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    sync.session()
        .unwrap()
        .append("Sent", b"hello", vec![Flag::Seen])
        .await
        .unwrap();
    let cmds = server.received.lock().await.clone();
    assert!(
        cmds.iter().any(|c| c.contains("APPEND")),
        "APPEND missing from: {cmds:?}"
    );
    assert!(
        cmds.iter().any(|c| c.contains("hello")),
        "literal bytes missing from: {cmds:?}"
    );
}

#[tokio::test]
async fn namespace_returns_prefix_groups() {
    let server = MockImapServer::start("IMAP4rev1 NAMESPACE", |tag, rest| {
        if rest.to_ascii_uppercase().starts_with("NAMESPACE") {
            vec![
                "* NAMESPACE ((\"INBOX.\" \".\")) NIL ((\"Shared/\" \"/\"))\r\n".to_string(),
                format!("{tag} OK NAMESPACE completed\r\n"),
            ]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;
    let account = test_mock_account(server.port);
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let (personal, other, shared) = sync.session().unwrap().namespace().await.unwrap();
    assert_eq!(personal, vec!["INBOX.".to_string()]);
    assert!(other.is_empty());
    assert_eq!(shared, vec!["Shared/".to_string()]);
}
