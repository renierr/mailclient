//! The life of a non-Inbox folder against a stateful mock server: quick
//! sync, opened (full window), "load older", a delete, and syncs after it.

use std::sync::{Arc, Mutex};

use crate::db::Db;
use crate::models::{FolderRole, NewAccount};
use crate::store::{accounts, folders, messages};
use crate::sync::imap::{
    mock::{test_mock_account, MockImapServer},
    ImapSync, FULL_SYNC_WINDOW, OLDER_BATCH, QUICK_SYNC_WINDOW,
};

/// Server state: live UIDs, UIDNEXT, HIGHESTMODSEQ.
type Mailbox = Arc<Mutex<(Vec<u32>, u32, u64)>>;

fn parse_set(s: &str, max: u32) -> Vec<u32> {
    let num = |p: &str| if p == "*" { max } else { p.parse().unwrap() };
    let mut out = Vec::new();
    for part in s.split(',').filter(|p| !p.is_empty()) {
        match part.split_once(':') {
            Some((a, b)) => {
                let (a, b) = (num(a), num(b));
                out.extend(a.min(b)..=a.max(b));
            }
            None => out.push(num(part)),
        }
    }
    out
}

fn raw(uid: u32) -> String {
    format!("From: a@example.com\r\nSubject: m{uid}\r\nContent-Type: text/plain\r\n\r\nbody {uid}")
}

/// Answers SELECT, UID SEARCH UID, UID FETCH and UID MOVE from `mailbox`.
/// With `sloppy_vanished` it is a QRESYNC server whose `VANISHED (EARLIER)`
/// covers everything from UID 1 up to the highest UID it ever expunged,
/// including mail it still holds.
async fn stateful_server(mailbox: Mailbox, sloppy_vanished: bool) -> MockImapServer {
    let caps = if sloppy_vanished {
        "IMAP4rev1 MOVE UIDPLUS CONDSTORE QRESYNC ENABLE"
    } else {
        "IMAP4rev1 MOVE UIDPLUS CONDSTORE"
    };
    let expunged_top = Arc::new(Mutex::new(0u32));
    MockImapServer::start(caps, move |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        let mut guard = mailbox.lock().unwrap();
        let (uids, next, modseq) = &mut *guard;
        let max = uids.iter().copied().max().unwrap_or(1);
        let arg = |n: usize| rest.split_whitespace().nth(n).unwrap_or("");
        if upper.starts_with("SELECT") {
            let top = *expunged_top.lock().unwrap();
            let mut out = Vec::new();
            if upper.contains("QRESYNC") && top > 0 {
                out.push(format!("* VANISHED (EARLIER) 1:{top}\r\n"));
            }
            out.extend([
                format!("* {} EXISTS\r\n", uids.len()),
                "* OK [UIDVALIDITY 1] Ok\r\n".to_string(),
                format!("* OK [UIDNEXT {next}] Ok\r\n"),
                format!("* OK [HIGHESTMODSEQ {modseq}] Ok\r\n"),
                format!("{tag} OK [READ-WRITE] SELECT completed\r\n"),
            ]);
            out
        } else if upper.starts_with("UID SEARCH") {
            let want = parse_set(rest.split_whitespace().last().unwrap(), max);
            let hits: Vec<String> = uids
                .iter()
                .filter(|u| want.contains(u))
                .map(u32::to_string)
                .collect();
            vec![
                format!("* SEARCH {}\r\n", hits.join(" ")),
                format!("{tag} OK UID SEARCH completed\r\n"),
            ]
        } else if upper.starts_with("UID FETCH") {
            let want = parse_set(arg(2), max);
            let mut out = Vec::new();
            for (i, u) in uids.iter().enumerate().filter(|(_, u)| want.contains(u)) {
                if upper.contains("BODY") {
                    let r = raw(*u);
                    out.push(format!(
                        "* {} FETCH (UID {u} FLAGS (\\Seen) BODY[] {{{}}}\r\n{r})\r\n",
                        i + 1,
                        r.len()
                    ));
                } else if !upper.contains("CHANGEDSINCE") {
                    out.push(format!("* {} FETCH (UID {u} FLAGS (\\Seen))\r\n", i + 1));
                }
            }
            out.push(format!("{tag} OK UID FETCH completed\r\n"));
            out
        } else if upper.starts_with("UID MOVE") {
            let gone = parse_set(arg(2), max);
            uids.retain(|u| !gone.contains(u));
            *modseq += 1;
            let mut top = expunged_top.lock().unwrap();
            *top = gone.iter().copied().max().unwrap_or(0).max(*top);
            vec![format!("{tag} OK MOVE completed\r\n")]
        } else {
            vec![format!("{tag} OK completed\r\n")]
        }
    })
    .await
}

#[tokio::test]
async fn a_quick_synced_folder_fills_on_open_and_keeps_backfilled_mail() {
    folder_life(false).await;
}

#[tokio::test]
async fn an_over_broad_vanished_report_does_not_drop_cached_mail() {
    folder_life(true).await;
}

async fn folder_life(sloppy_vanished: bool) {
    // A sparse Sent folder: 203 mails, UIDs with gaps, UIDNEXT well above.
    let mailbox: Mailbox = Arc::new(Mutex::new((
        (0..203).map(|i| 1 + i * 3).collect(),
        757,
        1000,
    )));
    let server = stateful_server(mailbox, sloppy_vanished).await;

    let db = Db::open_in_memory().unwrap();
    let account = test_mock_account(server.port);
    let account_id = accounts::create(
        &db,
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
    .unwrap();
    let sent = folders::upsert(&db, account_id, "Sent", "/", FolderRole::Sent).unwrap();
    folders::upsert(&db, account_id, "Trash", "/", FolderRole::Trash).unwrap();
    let cached = || messages::count_by_folder(&db, sent).unwrap();

    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();

    // Account sync: non-Inbox folders only get the quick window.
    sync.sync_folder_window(&db, sent, Some(QUICK_SYNC_WINDOW), None)
        .await
        .unwrap();
    assert_eq!(cached(), 50);

    // Opening the folder asks for the full window. The server has not
    // changed, but the cache is short of it: no unchanged fast path.
    sync.sync_folder_window(&db, sent, Some(FULL_SYNC_WINDOW), None)
        .await
        .unwrap();
    assert_eq!(cached(), 200);

    sync.sync_older(&db, sent, OLDER_BATCH).await.unwrap();
    assert_eq!(cached(), 203);

    sync.move_uids_to(&db, sent, &[301], "Trash").await.unwrap();
    assert_eq!(cached(), 202);

    // Neither the quick nor the full window may drop backfilled mail.
    for window in [QUICK_SYNC_WINDOW, FULL_SYNC_WINDOW] {
        let r = sync
            .sync_folder_window(&db, sent, Some(window), None)
            .await
            .unwrap();
        assert_eq!(r.expunged, 0);
        assert_eq!(cached(), 202);
    }
}
