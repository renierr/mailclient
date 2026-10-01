//! IDLE against the mock server: a server push ends the IDLE as a change,
//! the caller's wake ends it cleanly, a refusal is an error — and in every
//! clean case the session answers the next command.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::sync::imap::{
    mock::{test_mock_account, MockImapServer},
    IdleEnd, ImapSync,
};

/// A mock that accepts IDLE, optionally pushes `push` right away, and
/// completes the IDLE when the client sends DONE.
async fn idle_server(push: Option<&'static str>) -> MockImapServer {
    let idle_tag = Arc::new(Mutex::new(String::new()));
    MockImapServer::start("IMAP4rev1 IDLE", move |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("IDLE") {
            *idle_tag.lock().unwrap() = tag.to_string();
            let mut out = vec!["+ idling\r\n".to_string()];
            out.extend(push.map(str::to_string));
            out
        } else if tag.eq_ignore_ascii_case("DONE") {
            let tag = idle_tag.lock().unwrap().clone();
            vec![format!("{tag} OK IDLE terminated\r\n")]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await
}

#[tokio::test]
async fn new_mail_ends_the_idle_as_a_change() {
    let server = idle_server(Some("* 4 EXISTS\r\n")).await;
    let mut sync = ImapSync::new(&test_mock_account(server.port));
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    assert!(session.has_capability("IDLE"));

    let end = session.idle(std::future::pending()).await.unwrap();
    assert_eq!(end, IdleEnd::Changed);
    assert!(sync.is_healthy().await, "the session answers after DONE");
    let sent = server.received.lock().await;
    assert!(sent.iter().any(|c| c.ends_with("IDLE")));
    assert!(sent.iter().any(|c| c == "DONE"));
}

#[tokio::test]
async fn heartbeats_do_not_end_the_idle_but_the_wake_does() {
    let server = idle_server(Some("* OK Still here\r\n")).await;
    let mut sync = ImapSync::new(&test_mock_account(server.port));
    sync.connect("secret").await.unwrap();

    let wake = tokio::time::sleep(Duration::from_millis(100));
    let session = sync.session().unwrap();
    let end = session.idle(wake).await.unwrap();
    assert_eq!(end, IdleEnd::Woken);
    let stats = session.last_idle();
    assert_eq!(stats.heartbeats, 1);
    assert!(stats.heartbeat_every.is_some());
    assert!(sync.is_healthy().await);
}

#[tokio::test]
async fn a_refused_idle_is_an_error() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        if rest.to_ascii_uppercase().starts_with("IDLE") {
            vec![format!("{tag} BAD unknown command\r\n")]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await;
    let mut sync = ImapSync::new(&test_mock_account(server.port));
    sync.connect("secret").await.unwrap();
    let session = sync.session().unwrap();
    assert!(!session.has_capability("IDLE"));
    assert!(session.idle(std::future::pending()).await.is_err());
}

#[tokio::test]
async fn bye_while_idling_is_an_error() {
    let server = idle_server(Some("* BYE shutting down\r\n")).await;
    let mut sync = ImapSync::new(&test_mock_account(server.port));
    sync.connect("secret").await.unwrap();
    let end = sync.session().unwrap().idle(std::future::pending()).await;
    assert!(end.is_err());
}
