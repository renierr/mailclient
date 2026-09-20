//! mailfeed — Qt-free JSON feed CLI over `mailcore`.
//!
//! Machine interface for the Kotlin desktop frontend (`kotlin-desktop/`):
//! every command prints one JSON document to stdout (same shapes as the
//! QML feeds in `mailcore::feed`, so QML role names and Kotlin models agree).
//! Errors go to stderr with a non-zero exit code.
//!
//! ```sh
//! mailfeed accounts
//! mailfeed folders --account 1
//! mailfeed messages --folder 3 --limit 200 --offset 0
//! mailfeed message --folder 3 --uid 42
//! mailfeed search --account 1 --query invoice [--folder INBOX] [--limit 50]
//! mailfeed mark-read --folder 3 --uid 42 [--unread]
//! mailfeed mark-star --folder 3 --uid 42 [--off]
//! mailfeed status [--account 1]
//! mailfeed sync [--account 1]
//! ```
//!
//! `status` is cache-only (no network). `sync` syncs all accounts like
//! `mailapp --sync-once` (an `--account` value only scopes the report).
//! The DB is `mailcore::default_db_path()` (`MAILCLIENT_DB` override works).

use mailcore::store::{accounts, folders, messages, queue};
use mailcore::sync::headless;
use mailcore::{default_db_path, feed, Db};

fn resolve_secrets(acc: &mailcore::models::Account) -> (String, String) {
    if let Ok(secrets) = mailcore::auth::load_account_secrets(&acc.auth_vault_key) {
        if !secrets.imap_password.is_empty() {
            return (secrets.imap_password, secrets.smtp_password);
        }
    }
    let test_imap = std::env::var("MAILCLIENT_TEST_IMAP_PASS").unwrap_or_default();
    let test_smtp = std::env::var("MAILCLIENT_TEST_SMTP_PASS").unwrap_or_default();
    (test_imap, test_smtp)
}

fn usage() -> ! {
    eprintln!(
        "usage: mailfeed <accounts|folders|messages|message|search|mark-read|mark-star|status|sync|send|delete|archive|open-attachment> [options]\n\
         \n\
         \x20  accounts\n\
         \x20  folders --account <id>\n\
         \x20  messages --folder <id> [--limit <n>] [--offset <n>]\n\
         \x20  message --folder <id> --uid <n>\n\
         \x20  search --account <id> --query <q> [--folder <path>] [--limit <n>]\n\
         \x20  mark-read --folder <id> --uid <n> [--unread]\n\
         \x20  mark-star --folder <id> --uid <n> [--off]\n\
         \x20  status [--account <id>]\n\
         \x20  sync [--account <id>]\n\
         \x20  send --account <id> --to <to> [--cc <cc>] [--bcc <bcc>] --subject <s> --body <b> [--body-html <h>] [--reply-to <r>] [--attachments <f>]\n\
         \x20  delete --folder <id> --uid <n>\n\
         \x20  archive --folder <id> --uid <n>\n\
         \x20  open-attachment --id <n>"
    );
    std::process::exit(2);
}

/// Value of `--name <value>` / `--name=<value>`. Flag-only options are
/// detected with [`has_flag`].
fn opt(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(&format!("{name}=")) {
            return Some(v.to_string());
        }
    }
    None
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn parse_i64(args: &[String], name: &str, what: &str) -> i64 {
    match opt(args, name) {
        Some(v) => v.parse::<i64>().unwrap_or_else(|_| {
            eprintln!("{what}: expected integer for {name}, got '{v}'");
            std::process::exit(2);
        }),
        None => {
            eprintln!("{what}: missing required {name}");
            std::process::exit(2);
        }
    }
}

fn parse_u32(args: &[String], name: &str, what: &str) -> u32 {
    parse_i64(args, name, what).max(0) as u32
}

fn parse_u64_or(args: &[String], name: &str, default: u64) -> u64 {
    match opt(args, name) {
        Some(v) => v.parse::<u64>().unwrap_or_else(|_| {
            eprintln!("expected non-negative integer for {name}, got '{v}'");
            std::process::exit(2);
        }),
        None => default,
    }
}

fn open_db() -> Db {
    let path = default_db_path();
    Db::open(&path).unwrap_or_else(|e| {
        eprintln!("cannot open database at {}: {e}", path.display());
        std::process::exit(1);
    })
}

fn fail(e: impl std::fmt::Display) -> ! {
    eprintln!("error: {e}");
    std::process::exit(1);
}

fn main() {
    dotenvy::dotenv().ok();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let cmd = argv.first().map(|s| s.as_str());
    let rest = if argv.is_empty() { &[][..] } else { &argv[1..] };
    match cmd {
        Some("accounts") => {
            let db = open_db();
            match feed::accounts_json(&db) {
                Ok(j) => println!("{j}"),
                Err(e) => fail(e),
            }
        }
        Some("folders") => {
            let account = parse_i64(rest, "--account", "folders");
            let db = open_db();
            match feed::folders_json(&db, account) {
                Ok(j) => println!("{j}"),
                Err(e) => fail(e),
            }
        }
        Some("messages") => {
            let folder = parse_i64(rest, "--folder", "messages");
            let limit = parse_u64_or(rest, "--limit", feed::FEED_LIMIT);
            let offset = parse_u64_or(rest, "--offset", 0);
            let db = open_db();
            // Compact rows (no bodies) like the QML list path; the reader
            // payload comes from `message`.
            match feed::messages_list_json_paged(&db, folder, limit, offset) {
                Ok(j) => println!("{j}"),
                Err(e) => fail(e),
            }
        }
        Some("message") => {
            let folder = parse_i64(rest, "--folder", "message");
            let uid = parse_u32(rest, "--uid", "message");
            let db = open_db();
            match feed::message_json(&db, folder, uid) {
                Ok(j) => println!("{j}"),
                Err(e) => fail(e),
            }
        }
        Some("search") => {
            let account = parse_i64(rest, "--account", "search");
            let query = opt(rest, "--query").unwrap_or_else(|| {
                eprintln!("search: missing required --query");
                std::process::exit(2);
            });
            let folder = opt(rest, "--folder").unwrap_or_default();
            let limit = parse_u64_or(rest, "--limit", 50);
            let db = open_db();
            match feed::search_json(&db, account, &query, limit, &folder) {
                Ok(j) => println!("{j}"),
                Err(e) => fail(e),
            }
        }
        Some("mark-read") => {
            let folder = parse_i64(rest, "--folder", "mark-read");
            let uid = parse_u32(rest, "--uid", "mark-read");
            let read = !has_flag(rest, "--unread");
            let db = open_db();
            // Local-only + flags_dirty, like the QML click path: the next
            // sync pushes the flag to IMAP.
            match messages::set_read_many_by_uids(&db, folder, &[uid], read) {
                Ok(n) => println!("{{\"ok\":true,\"updated\":{n}}}"),
                Err(e) => fail(e),
            }
        }
        Some("mark-star") => {
            let folder = parse_i64(rest, "--folder", "mark-star");
            let uid = parse_u32(rest, "--uid", "mark-star");
            let starred = !has_flag(rest, "--off");
            let db = open_db();
            match messages::set_star_many_by_uids(&db, folder, &[uid], starred) {
                Ok(n) => println!("{{\"ok\":true,\"updated\":{n}}}"),
                Err(e) => fail(e),
            }
        }
        Some("status") => {
            let filter = opt(rest, "--account").and_then(|v| v.parse::<i64>().ok());
            let db = open_db();
            let mut accounts = headless::unread_summary(&db);
            if let Some(id) = filter {
                accounts.retain(|a| a.account_id == id);
            }
            let unread: u64 = accounts.iter().map(|a| a.unread).sum();
            let recent = headless::recent_unread(&db, 10, filter);
            let payload = serde_json::json!({
                "ok": true,
                "unread": unread,
                "accounts": accounts,
                "recent": recent,
            });
            println!("{payload}");
        }
        Some("sync") => {
            let filter = opt(rest, "--account").and_then(|v| v.parse::<i64>().ok());
            let db_path = default_db_path();
            let db = open_db();
            // Same cross-process guard as `mailapp --sync-once`: a live GUI
            // holder means the cache is fresh anyway.
            let _guard = match headless::acquire_sync_lock(&db_path) {
                Ok(g) => g,
                Err(e) => fail(format!("sync lock: {e}")),
            };
            if _guard.is_none() {
                println!("{{\"ok\":true,\"locked\":true}}");
                return;
            }
            let mut report = headless::sync_all_accounts_blocking(&db);
            if let Some(id) = filter {
                report.accounts.retain(|a| a.account_id == id);
                report.total_unread = report.accounts.iter().map(|a| a.unread).sum();
                report.total_fetched = report.accounts.iter().map(|a| a.fetched).sum();
                report.total_expunged = report.accounts.iter().map(|a| a.expunged).sum();
            }
            let payload = serde_json::json!({
                "ok": true,
                "locked": false,
                "unread": report.total_unread,
                "fetched": report.total_fetched,
                "expunged": report.total_expunged,
                "accounts": report.accounts,
                "errors": report.errors,
            });
            println!("{payload}");
        }
        Some("send") => {
            let acc_id = parse_i64(rest, "--account", "send");
            let to_raw = opt(rest, "--to").unwrap_or_default();
            let cc_raw = opt(rest, "--cc").unwrap_or_default();
            let bcc_raw = opt(rest, "--bcc").unwrap_or_default();
            let subject = opt(rest, "--subject").unwrap_or_default();
            let body = opt(rest, "--body").unwrap_or_default();
            let body_html = opt(rest, "--body-html");
            let reply_to = opt(rest, "--reply-to");
            let attachments_raw = opt(rest, "--attachments").unwrap_or_default();

            let to: Vec<String> = to_raw.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let cc: Vec<String> = cc_raw.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let bcc: Vec<String> = bcc_raw.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            let attachments: Vec<String> = attachments_raw.split([';', '\n']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

            if to.is_empty() && cc.is_empty() && bcc.is_empty() {
                fail("add at least one recipient (To, Cc or Bcc)");
            }

            let db = open_db();
            let acc = match accounts::get(&db, acc_id) {
                Ok(a) => a,
                Err(e) => fail(format!("account not found: {e}")),
            };
            let (imap_pw, smtp_pw) = resolve_secrets(&acc);
            if smtp_pw.is_empty() {
                fail("no SMTP password available in keyring or .env");
            }

            let sender = mailcore::sync::sender::SmtpSender::new(&acc);
            let from_name = (!acc.from_name.trim().is_empty()).then_some(acc.from_name.as_str());
            let req = mailcore::sync::sender::SendRequest {
                to: &to,
                cc: &cc,
                bcc: &bcc,
                from: Some(&acc.email_address),
                from_name,
                reply_to: reply_to.as_deref(),
                subject: &subject,
                body_text: &body,
                body_html: body_html.as_deref(),
                attachments: &attachments,
                format: mailcore::sync::sender::SendFormat::Auto,
                include_plain: true,
                policy: &mailcore::sync::sender::SendPolicy::Unrestricted,
                password: "",
                imap_password: None,
                request_mdn: false,
            };

            let (queue_id, raw) = match sender.enqueue_send(&db, acc.id, &req) {
                Ok(v) => v,
                Err(e) => fail(format!("cannot enqueue send: {e}")),
            };

            if let Err(e) = sender.submit_queued(&db, queue_id, &smtp_pw) {
                let _ = queue::discard_mime(&db, queue_id);
                fail(format!("SMTP submit failed: {e}"));
            }

            let rt = tokio::runtime::Runtime::new().unwrap();
            let imap_pw_ref = if imap_pw.is_empty() { None } else { Some(imap_pw.as_str()) };
            let _ = rt.block_on(async {
                sender.save_sent_copy(&db, acc.id, imap_pw_ref, &raw).await
            });

            println!("{{\"ok\":true,\"message\":\"Mail sent successfully\"}}");
        }
        Some("delete") => {
            let folder_id = parse_i64(rest, "--folder", "delete");
            let uid = parse_u32(rest, "--uid", "delete");
            let db = open_db();
            let msg = match messages::get_by_uid(&db, folder_id, uid) {
                Ok(m) => m,
                Err(e) => fail(format!("message not found: {e}")),
            };
            let folder = match folders::get(&db, msg.folder_id) {
                Ok(f) => f,
                Err(e) => fail(format!("folder not found: {e}")),
            };
            let acc = match accounts::get(&db, folder.account_id) {
                Ok(a) => a,
                Err(e) => fail(format!("account not found: {e}")),
            };
            let (imap_pw, _) = resolve_secrets(&acc);
            let rt = tokio::runtime::Runtime::new().unwrap();
            let outcome = rt.block_on(async {
                if !imap_pw.is_empty() {
                    let mut imap = mailcore::sync::imap::ImapSync::new(&acc);
                    if imap.connect(&imap_pw).await.is_ok() {
                        if let Ok(outcome) = imap.trash_message(&db, msg.id).await {
                            return format!("{outcome:?}");
                        }
                    }
                }
                let _ = messages::delete(&db, msg.id);
                "DeletedLocally".to_string()
            });
            println!("{{\"ok\":true,\"outcome\":\"{outcome}\"}}");
        }
        Some("archive") => {
            let folder_id = parse_i64(rest, "--folder", "archive");
            let uid = parse_u32(rest, "--uid", "archive");
            let db = open_db();
            let msg = match messages::get_by_uid(&db, folder_id, uid) {
                Ok(m) => m,
                Err(e) => fail(format!("message not found: {e}")),
            };
            let folder = match folders::get(&db, msg.folder_id) {
                Ok(f) => f,
                Err(e) => fail(format!("folder not found: {e}")),
            };
            let acc = match accounts::get(&db, folder.account_id) {
                Ok(a) => a,
                Err(e) => fail(format!("account not found: {e}")),
            };
            let (imap_pw, _) = resolve_secrets(&acc);
            let rt = tokio::runtime::Runtime::new().unwrap();
            let outcome = rt.block_on(async {
                if !imap_pw.is_empty() {
                    let mut imap = mailcore::sync::imap::ImapSync::new(&acc);
                    if imap.connect(&imap_pw).await.is_ok() {
                        if let Ok(outcome) = imap.archive_message(&db, msg.id).await {
                            return format!("{outcome:?}");
                        }
                    }
                }
                let _ = messages::delete(&db, msg.id);
                "ArchivedLocally".to_string()
            });
            println!("{{\"ok\":true,\"outcome\":\"{outcome}\"}}");
        }
        Some("open-attachment") => {
            let id = parse_i64(rest, "--id", "open-attachment");
            let db = open_db();
            let att = match messages::get_attachment(&db, id) {
                Ok(a) => a,
                Err(e) => fail(format!("attachment not found: {e}")),
            };
            let msg = match messages::get(&db, att.message_id) {
                Ok(m) => m,
                Err(e) => fail(format!("parent message not found: {e}")),
            };
            let folder = match folders::get(&db, msg.folder_id) {
                Ok(f) => f,
                Err(e) => fail(format!("folder not found: {e}")),
            };
            let acc = match accounts::get(&db, folder.account_id) {
                Ok(a) => a,
                Err(e) => fail(format!("account not found: {e}")),
            };
            let (imap_pw, _) = resolve_secrets(&acc);
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let has_data = messages::attachment_has_data(&db, id).unwrap_or(false);
                if !has_data && !imap_pw.is_empty() {
                    let mut imap = mailcore::sync::imap::ImapSync::new(&acc);
                    if imap.connect(&imap_pw).await.is_ok() {
                        let _ = imap.fetch_attachments(&db, msg.id).await;
                    }
                }
            });
            let temp_dir = std::env::temp_dir().join("mailclient-attachments");
            let _ = std::fs::create_dir_all(&temp_dir);
            let safe_name = att.filename.as_deref().unwrap_or("attachment").replace(['/', '\\', '\0'], "_");
            let dest = temp_dir.join(format!("{}-{}-{}", att.message_id, att.id, safe_name));
            match messages::save_attachment_to_path(&db, id, &dest) {
                Ok(_) => {
                    let path_str = dest.to_string_lossy();
                    println!("{{\"ok\":true,\"path\":{}}}", serde_json::to_string(&path_str).unwrap());
                }
                Err(e) => fail(format!("cannot save attachment: {e}")),
            }
        }
        _ => usage(),
    }
}
