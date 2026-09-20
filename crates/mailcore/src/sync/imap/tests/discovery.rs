//! Discovery: dotted-subtree + NAMESPACE passes and parent folder creation.

use crate::db::Db;
use crate::store::accounts;
use crate::sync::imap::{
    mock::{test_mock_account, MockImapServer},
    ImapSync,
};
use crate::sync::traits::SyncProvider;

#[tokio::test]
async fn discovery_finds_dotted_subtree_and_shared_namespace() {
    let server = MockImapServer::start("IMAP4rev1 NAMESPACE", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("NAMESPACE") {
            return vec![
                "* NAMESPACE NIL NIL ((\"Shared/\" \"/\"))\r\n".to_string(),
                format!("{tag} OK NAMESPACE completed\r\n"),
            ];
        }
        if upper.starts_with("LIST") {
            // Roots for pass 3.
            if rest.contains('"') && rest.contains('%') {
                return vec![
                    "* LIST (\\HasNoChildren) \"/\" INBOX\r\n".to_string(),
                    format!("{tag} OK LIST completed\r\n"),
                ];
            }
            // Dotted Tobit-style child (pass 3) and shared branch (pass 4).
            if rest.contains("INBOX.") {
                return vec![
                    "* LIST (\\HasNoChildren) \".\" INBOX.Archive\r\n".to_string(),
                    format!("{tag} OK LIST completed\r\n"),
                ];
            }
            if rest.contains("Shared/") {
                return vec![
                    "* LIST (\\HasNoChildren) \"/\" Shared/Team\r\n".to_string(),
                    format!("{tag} OK LIST completed\r\n"),
                ];
            }
            return vec![
                "* LIST (\\HasNoChildren) \"/\" INBOX\r\n".to_string(),
                format!("{tag} OK LIST completed\r\n"),
            ];
        }
        if upper.starts_with("LSUB") {
            return vec![format!("{tag} OK LSUB completed\r\n")];
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
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let found = sync.sync_folders(&db, account_id).await.unwrap();
    let paths: Vec<String> = found.into_iter().map(|f| f.path).collect();
    assert!(
        paths.contains(&"INBOX.Archive".to_string()),
        "dotted subtree missing: {paths:?}"
    );
    assert!(
        paths.contains(&"Shared/Team".to_string()),
        "shared namespace missing: {paths:?}"
    );
}

#[tokio::test]
async fn create_folder_path_creates_parents_and_discovers() {
    let server = MockImapServer::start("IMAP4rev1", |tag, rest| {
        let upper = rest.to_ascii_uppercase();
        if upper.starts_with("CREATE") {
            return vec![format!("{tag} OK CREATE completed\r\n")];
        }
        if upper.starts_with("LIST") {
            return vec![
                "* LIST (\\HasNoChildren) \"/\" Work\r\n".to_string(),
                "* LIST (\\HasNoChildren) \"/\" Work/Client\r\n".to_string(),
                format!("{tag} OK LIST completed\r\n"),
            ];
        }
        if upper.starts_with("LSUB") {
            return vec![format!("{tag} OK LSUB completed\r\n")];
        }
        if upper.starts_with("NAMESPACE") {
            return vec![
                "* NAMESPACE NIL NIL NIL\r\n".to_string(),
                format!("{tag} OK NAMESPACE completed\r\n"),
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
    let mut sync = ImapSync::new(&account);
    sync.connect("secret").await.unwrap();
    let folder = sync
        .create_folder_path(&db, account_id, "Work/Client", "/")
        .await
        .unwrap();
    assert_eq!(folder.path, "Work/Client");
    let cmds = server.received.lock().await.clone();
    let creates: Vec<&String> = cmds
        .iter()
        .filter(|c| c.to_ascii_uppercase().contains("CREATE"))
        .collect();
    assert!(
        creates
            .iter()
            .any(|c| c.contains("Work\"") || c.ends_with("Work")),
        "parent CREATE missing: {cmds:?}"
    );
    assert!(
        creates.iter().any(|c| c.contains("Work/Client")),
        "child CREATE missing: {cmds:?}"
    );
}
