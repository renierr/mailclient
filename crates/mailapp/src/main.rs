//! mailapp — Qt/QML shell over `mailcore`.
//!
//! Boots a `QGuiApplication` + `QQmlApplicationEngine`, opens (and migrates)
//! the SQLite DB, then loads `Main.qml`:
//!
//! 1. `$MAILCLIENT_QML_DIR/Main.qml` (explicit override, designer iteration)
//! 2. `<exe>/../share/mailclient/qml/Main.qml` (installed: `~/.local/...`)
//! 3. `<exe>/../qml/Main.qml` (dist bundle: `dist/mailclient/...`)
//! 4. Embedded `Mailclient` QML module
//!    (`qrc:/qt/qml/Mailclient/qml/Main.qml`, always available)
//!
//! Rust QObjects (`Mailclient` module) are registered in the binary, so
//! `import Mailclient` resolves no matter where `Main.qml` loads from.

pub mod bridge;
pub mod platform;

use std::path::PathBuf;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QQuickStyle, QString, QUrl};
use mailcore::{default_db_path, Db};

/// Filesystem candidates for `Main.qml` (see module docs).
fn find_main_qml() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("MAILCLIENT_QML_DIR") {
        let p = PathBuf::from(dir).join("Main.qml");
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [
                dir.join("../share/mailclient/qml/Main.qml"),
                dir.join("../qml/Main.qml"),
            ] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn main() {
    env_logger::init();

    // Headless mode for the Omarchy bar widget / systemd timer. Same binary,
    // no Qt: sync all accounts (or one) and report unread counts as JSON.
    // Unknown `--` flags fail fast instead of booting the GUI by accident
    // (a typo in a bar script must not pop a window).
    let cli: Vec<String> = std::env::args().skip(1).collect();
    if cli.iter().any(|a| a == "--sync-once" || a == "--status" || a == "--help") {
        std::process::exit(run_headless(&cli));
    }
    if let Some(bad) = cli.iter().find(|a| a.starts_with("--")) {
        eprintln!("unknown option '{bad}': usage: mailapp [--status | --sync-once [--account <id|email>]] [--json]");
        std::process::exit(2);
    }

    // Open (creating + migrating) the local cache DB before UI startup so
    // first paint already knows the DB path; models attach in M1.
    let db_path = default_db_path();
    match Db::open(&db_path) {
        Ok(_) => log::info!("opened database at {}", db_path.display()),
        Err(e) => log::error!("cannot open database at {}: {e}", db_path.display()),
    }

    let url = match find_main_qml() {
        Some(qml) => {
            log::info!("loading QML from {}", qml.display());
            let abs = std::path::absolute(&qml).unwrap_or(qml);
            // `format!("file://{abs}")` breaks on Windows: the drive letter
            // parses as the URL scheme and backslashes are not separators, so
            // Qt treats it as a remote host. QUrl::fromLocalFile does the
            // platform-correct encoding.
            QUrl::from_local_file(&QString::from(abs.display().to_string().as_str()))
        }
        None => {
            log::info!("loading embedded QML module");
            QUrl::from("qrc:/qt/qml/Mailclient/qml/Main.qml")
        }
    };

    // Pin the Controls style before any QML loads. Without this Qt picks the
    // platform default -- the native Windows style, which ignores most
    // customisation and looks nothing like the Linux build. Basic applies no
    // styling of its own, so qml/Theme.qml is fully in charge and both
    // platforms render identically.
    QQuickStyle::set_style(&QString::from("Basic"));

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&url);
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

/// Headless entry point: `--status` (cached unread, no network) or
/// `--sync-once` (sync, then report). Prints JSON with `--json`, otherwise a
/// one-line human summary. Exit 0 on success (per-account errors ride along
/// in the payload), 1 on fatal failure, 2 on usage error.
fn run_headless(args: &[String]) -> i32 {
    use mailcore::sync::headless;
    use mailcore::{default_db_path, Db};

    let sync = args.iter().any(|a| a == "--sync-once");
    let json = args.iter().any(|a| a == "--json");
    let filter = args
        .windows(2)
        .find(|w| w[0] == "--account")
        .map(|w| w[1].clone());
    if args.iter().any(|a| {
        a.starts_with('-')
            && a != "--sync-once"
            && a != "--status"
            && a != "--json"
            && a != "--account"
    }) {
        eprintln!("usage: mailapp [--status | --sync-once [--account <id|email>]] [--json]");
        return 2;
    }

    let db_path = default_db_path();
    let db = match Db::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot open database at {}: {e}", db_path.display());
            return 1;
        }
    };

    // Sync mode takes the cross-process lock; a live holder (open GUI
    // mid-sync) means the cache is fresh anyway, so report it as locked.
    let mut locked = false;
    let _guard = if sync {
        match headless::acquire_sync_lock(&db_path) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("sync lock: {e}");
                return 1;
            }
        }
    } else {
        None
    };
    if sync && _guard.is_none() {
        locked = true;
    }

    let mut report = if sync && !locked {
        match &filter {
            Some(f) => {
                let acc = mailcore::store::accounts::list(&db)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|a| a.id.to_string() == *f || a.email_address == *f);
                match acc {
                    Some(a) => {
                        let mut imap = mailcore::sync::imap::ImapSync::new(&a);
                        let mut r = headless::SyncAllReport::default();
                        match mailcore::auth::load_account_secrets(&a.auth_vault_key)
                            .map_err(|e| e.to_string())
                            .and_then(|s| {
                                imap.connect(&s.imap_password).map_err(|e| e.to_string())?;
                                Ok((s, headless::sync_account(&db, &a, &mut imap)))
                            }) {
                            Ok((_, ar)) => {
                                imap.disconnect();
                                r.total_unread = ar.unread;
                                r.errors.extend(
                                    ar.errors
                                        .iter()
                                        .map(|e| format!("{}: {e}", a.email_address)),
                                );
                                r.accounts.push(ar);
                            }
                            Err(e) => {
                                let mut ar = headless::AccountSyncResult {
                                    account_id: a.id,
                                    email: a.email_address.clone(),
                                    ..Default::default()
                                };
                                ar.errors.push(e);
                                ar.unread = headless::unread_for_account(&db, a.id);
                                r.total_unread = ar.unread;
                                r.errors.extend(
                                    ar.errors
                                        .iter()
                                        .map(|e| format!("{}: {e}", a.email_address)),
                                );
                                r.accounts.push(ar);
                            }
                        }
                        r
                    }
                    None => {
                        eprintln!("no account matching '{f}'");
                        return 2;
                    }
                }
            }
            None => headless::sync_all_accounts(&db),
        }
    } else {
        let accounts = headless::unread_summary(&db);
        let total_unread = accounts.iter().map(|a| a.unread).sum();
        headless::SyncAllReport {
            accounts,
            total_unread,
            ..Default::default()
        }
    };

    // Keep totals consistent for the single-account path.
    report.total_fetched = report.accounts.iter().map(|a| a.fetched).sum();
    report.total_expunged = report.accounts.iter().map(|a| a.expunged).sum();
    if report.total_unread == 0 {
        report.total_unread = report.accounts.iter().map(|a| a.unread).sum();
    }
    let recent = headless::recent_unread(&db, 10);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if json {
        let payload = serde_json::json!({
            "ok": true,
            "mode": if sync && !locked { "sync" } else { "status" },
            "locked": locked,
            "synced_at": now,
            "unread": report.total_unread,
            "accounts": report.accounts,
            "recent": recent,
            "errors": report.errors,
        });
        println!("{}", payload);
    } else if report.total_unread == 0 {
        println!(
            "mail: no unread mail{}",
            if locked { " (sync locked)" } else { "" }
        );
    } else {
        println!(
            "mail: {} unread across {} account(s){}",
            report.total_unread,
            report.accounts.len(),
            if locked { " (sync locked)" } else { "" }
        );
        for m in &recent {
            println!("  [{}] {} — {}", m.account_email, m.from, m.subject);
        }
    }
    0
}
