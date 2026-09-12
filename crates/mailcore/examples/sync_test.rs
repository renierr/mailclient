//! M1 test harness: sync the `.env` test account into a scratch SQLite DB.
//!
//! NEVER runs against the live server unless explicitly asked, every time:
//! ```sh
//! cargo run -p mailcore --example sync_test -- --live            # sync only
//! MAILCLIENT_SEND_TEST_MAIL=1 ... -- --live                     # + one test mail
//! ```
//! Without `--live` the harness exits immediately. `cargo test` never touches
//! the network (unit tests use in-memory SQLite only).
//!
//! Read-only by default. Sending happens ONLY when `MAILCLIENT_SEND_TEST_MAIL=1`
//! is set alongside `--live`, and even then only to the [`SendPolicy`]
//! allowlist (`MAILCLIENT_TEST_SEND_ALLOWLIST`, empty by default = deny all).

use std::env;

use mailcore::db::Db;
use mailcore::models::NewAccount;
use mailcore::store::{accounts, folders, messages};
use mailcore::sync::imap::ImapSync;
use mailcore::sync::sender::{SendFormat, SendPolicy, SendRequest, SmtpSender};
use mailcore::sync::traits::{MailSender, SyncProvider};

fn var(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("missing env var {name} (see .env.example)"))
}

fn main() -> Result<(), String> {
    if !env::args().any(|a| a == "--live") {
        return Err(
            "refusing live run without explicit consent: re-run with `-- --live`".to_string(),
        );
    }
    dotenvy::dotenv().ok();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let email = var("MAILCLIENT_TEST_EMAIL")?;
    let imap_host = var("MAILCLIENT_TEST_IMAP_HOST")?;
    let imap_port: u16 = var("MAILCLIENT_TEST_IMAP_PORT")?
        .parse()
        .map_err(|_| "MAILCLIENT_TEST_IMAP_PORT must be a number".to_string())?;
    let imap_security =
        env::var("MAILCLIENT_TEST_IMAP_SECURITY").unwrap_or_else(|_| "tls".to_string());
    let imap_user = var("MAILCLIENT_TEST_IMAP_USER")?;
    let imap_pass = var("MAILCLIENT_TEST_IMAP_PASS")?;
    let smtp_host = var("MAILCLIENT_TEST_SMTP_HOST")?;
    let smtp_port: u16 = var("MAILCLIENT_TEST_SMTP_PORT")?
        .parse()
        .map_err(|_| "MAILCLIENT_TEST_SMTP_PORT must be a number".to_string())?;
    let smtp_security =
        env::var("MAILCLIENT_TEST_SMTP_SECURITY").unwrap_or_else(|_| "tls".to_string());
    let smtp_user = var("MAILCLIENT_TEST_SMTP_USER")?;
    let smtp_pass = var("MAILCLIENT_TEST_SMTP_PASS")?;

    let db_path =
        env::var("MAILCLIENT_DB").unwrap_or_else(|_| "target/sync-test.sqlite".to_string());
    let db = Db::open(std::path::Path::new(&db_path)).map_err(|e| e.to_string())?;
    println!("db: {db_path}");

    // Ensure the test account row.
    let account_id = match accounts::list(&db)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.email_address == email)
    {
        Some(a) => {
            println!("account: {} (existing id {})", a.email_address, a.id);
            a.id
        }
        None => {
            let id = accounts::create(
                &db,
                &NewAccount {
                    name: "Test".to_string(),
                    email_address: email.clone(),
                    from_name: String::new(),
                    imap_host: imap_host.clone(),
                    imap_port,
                    imap_security: imap_security.clone(),
                    imap_username: imap_user.clone(),
                    smtp_host: smtp_host.clone(),
                    smtp_port,
                    smtp_security: smtp_security.clone(),
                    smtp_username: smtp_user.clone(),
                    auth_vault_key: "env:test".to_string(),
                    check_interval_secs: 300,
                },
            )
            .map_err(|e| e.to_string())?;
            println!("account: {email} (created id {id})");
            id
        }
    };
    let account = accounts::get(&db, account_id).map_err(|e| e.to_string())?;

    // IMAP sync.
    let mut imap = ImapSync::new(&account);
    imap.connect(&imap_pass).map_err(|e| e.to_string())?;
    let synced = imap
        .sync_folders(&db, account_id)
        .map_err(|e| e.to_string())?;
    println!("--- folders ({}) ---", synced.len());
    for f in folders::list_by_account(&db, account_id).map_err(|e| e.to_string())? {
        println!(
            "  [{}] {} (role={}, uid_validity={:?})",
            f.id,
            f.path,
            f.role.as_str(),
            f.uid_validity
        );
    }
    println!("--- messages ---");
    for f in synced {
        let report = imap.sync_folder(&db, f.id).map_err(|e| e.to_string())?;
        let unread = messages::count_unread(&db, f.id).map_err(|e| e.to_string())?;
        println!(
            "  {}: +{} fetched, -{} expunged, {} unread",
            f.path, report.fetched, report.expunged, unread
        );
        for m in messages::list_by_folder(&db, f.id, 5, 0).map_err(|e| e.to_string())? {
            println!(
                "    uid={} read={} {} — {}",
                m.uid,
                m.is_read,
                m.from_addr.as_deref().unwrap_or("?"),
                m.subject.as_deref().unwrap_or("(no subject)")
            );
        }
    }
    imap.disconnect();

    // Optional: exactly one test mail, allowlist-enforced.
    if env::var("MAILCLIENT_SEND_TEST_MAIL").as_deref() == Ok("1") {
        let to = var("MAILCLIENT_TEST_SEND_TO")?;
        let policy = SendPolicy::from_env();
        let mut sender = SmtpSender::new(&account);
        sender
            .send_raw(
                &db,
                account_id,
                &SendRequest {
                    to: std::slice::from_ref(&to),
                    cc: &[],
                    bcc: &[],
                    from: None,
                    subject: "Mailclient M1 test",
                    body_text:
                        "Hello from the mailclient M1 sync harness. If you read this, SMTP works.",
                    body_html: None,
                    attachments: &[],
                    format: SendFormat::Plain,
                    include_plain: true,
                    from_name: None,
                    policy: &policy,
                    password: &smtp_pass,
                    imap_password: Some(&imap_pass),
                },
            )
            .map_err(|e| e.to_string())?;
        println!("sent test mail to {to}");
    } else {
        println!("(set MAILCLIENT_SEND_TEST_MAIL=1 to send one allowlisted test mail)");
    }

    Ok(())
}
