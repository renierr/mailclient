#![cfg_attr(windows, windows_subsystem = "windows")]
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
//!
//! Windows gives a console-subsystem binary a terminal window of its own, so
//! launching the exe from Explorer or a shortcut flashed one up beside the
//! GUI. Hence the `windows_subsystem` attribute above. It also cuts the
//! headless CLI off from the shell that started it, which
//! [`platform::attach_parent_console`] undoes when there is a console to
//! reconnect to. Neither concept exists on the other platforms.

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
    // Before anything prints: on Windows this is what gives `--status`,
    // `--sync-once` and the log a console to write to (see the module docs).
    platform::attach_parent_console();
    env_logger::init();

    // Headless mode for the Omarchy bar widget / systemd timer. Same binary,
    // no Qt: sync all accounts (or one) and report unread counts as JSON.
    // Unknown `--` flags fail fast instead of booting the GUI by accident
    // (a typo in a bar script must not pop a window).
    let cli: Vec<String> = std::env::args().skip(1).collect();
    if cli
        .iter()
        .any(|a| a == "--sync-once" || a == "--status" || a == "--help")
    {
        std::process::exit(run_headless(&cli));
    }
    // Jump request from the bar widget / a notification: queue the target
    // (account + folder) for the GUI and hand over. A live GUI picks it up
    // on its poll timer; otherwise this same process boots it below.
    let open_pos = cli.iter().position(|a| a == "--open");
    if let Some(pos) = open_pos {
        let code = run_open(&cli, pos);
        // 3 = queued but no live window: fall through and boot the GUI,
        // which consumes the queued request on startup.
        if code != 3 {
            std::process::exit(code);
        }
    }
    // `--open` operands are known args, never unknown flags — including on
    // the fall-through above, where `--open` itself would trip this check.
    if let Some(bad) = cli
        .iter()
        .enumerate()
        .filter(|(i, _)| !is_open_operand(&cli, open_pos, *i))
        .map(|(_, a)| a)
        .find(|a| a.starts_with("--"))
    {
        eprintln!("unknown option '{bad}': usage: mailapp [--status | --sync-once [--account <id|email>] | --open <id|email> [folder]] [--json]");
        std::process::exit(2);
    }

    // Open (creating + migrating) the local cache DB before UI startup so
    // first paint already knows the DB path; models attach in M1.
    let db_path = default_db_path();
    match Db::open(&db_path) {
        Ok(_) => log::info!("opened database at {}", db_path.display()),
        Err(e) => log::error!("cannot open database at {}: {e}", db_path.display()),
    }
    // Single-instance relay for `--open` clicks: while this window lives,
    // the pid file tells jump requests to exit after queueing (the running
    // window picks them up on its poll timer). Stale after a crash — the
    // liveness check in `run_open` covers that, and this overwrites it.
    let gui_pid = gui_pid_path(&db_path);
    if let Some(parent) = gui_pid.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&gui_pid, std::process::id().to_string());

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
    // Window gone: pid file must not outlive us, or later `--open` clicks
    // would queue into the void instead of booting a fresh window.
    let _ = std::fs::remove_file(&gui_pid);
}

/// Headless entry point: `--status` (cached unread, no network) or
/// `--sync-once` (sync, then report). Prints JSON with `--json`, otherwise a
/// one-line human summary. Exit 0 on success (per-account errors ride along
/// in the payload), 1 on fatal failure, 2 on usage error.
///
/// Jump request for the GUI: `mailapp --open <id|email> [folder]`. Queues
/// the target account (+ folder, defaulting to that account's inbox) for
/// the GUI and exits 0 when a live window will pick it up; otherwise
/// returns 3 so the caller falls through to booting the GUI itself.
fn run_open(cli: &[String], pos: usize) -> i32 {
    use mailcore::store::{accounts, folders, settings};

    let usage = "usage: mailapp --open <id|email> [folder]";
    let Some(value) = cli.get(pos + 1).filter(|v| !v.starts_with("--")) else {
        eprintln!("{usage}");
        return 2;
    };
    let folder_arg = cli
        .get(pos + 2)
        .filter(|v| !v.starts_with("--"))
        .map(|s| s.as_str());

    let db_path = default_db_path();
    let db = match Db::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot open database at {}: {e}", db_path.display());
            return 1;
        }
    };
    let Some(acc) = accounts::list(&db)
        .unwrap_or_default()
        .into_iter()
        .find(|a| a.id.to_string() == *value || a.email_address == *value)
    else {
        eprintln!("no account matching '{value}'");
        return 2;
    };
    let account_folders = folders::list_by_account(&db, acc.id).unwrap_or_default();
    let folder = match folder_arg {
        Some(f)
            if account_folders.iter().any(|known| {
                known.path == f
                    || known.path.eq_ignore_ascii_case("INBOX") && f.eq_ignore_ascii_case("inbox")
            }) =>
        {
            // Exact path wins; a bare "inbox"/"INBOX" resolves to the real
            // inbox path even when the server spells it differently.
            account_folders
                .iter()
                .find(|known| known.path == f)
                .map(|known| known.path.clone())
                .unwrap_or_else(|| {
                    account_folders
                        .iter()
                        .find(|known| known.role == mailcore::models::FolderRole::Inbox)
                        .map(|known| known.path.clone())
                        .unwrap_or_else(|| "INBOX".to_string())
                })
        }
        _ => account_folders
            .iter()
            .find(|known| known.role == mailcore::models::FolderRole::Inbox)
            .map(|known| known.path.clone())
            .unwrap_or_else(|| "INBOX".to_string()),
    };
    if let Err(e) = settings::set_pending_open(&db, acc.id, &folder) {
        eprintln!("cannot queue open request: {e}");
        return 1;
    }
    println!("mail: opening {} ({})", acc.email_address, folder);
    if gui_is_alive(&db_path) {
        0
    } else {
        3
    }
}

/// Whether `cli[idx]` belongs to the `--open` group at `open_pos` (the
/// flag itself plus its account value and optional folder), mirroring the
/// parsing in [`run_open`]. Keeps the unknown-flag check from tripping on
/// the boot fall-through, where `--open` is a known arg.
fn is_open_operand(cli: &[String], open_pos: Option<usize>, idx: usize) -> bool {
    let Some(pos) = open_pos else {
        return false;
    };
    if idx == pos {
        return true;
    }
    if idx == pos + 1 && cli.get(idx).is_some_and(|v| !v.starts_with("--")) {
        return true;
    }
    idx == pos + 2
        && cli.get(pos + 1).is_some_and(|v| !v.starts_with("--"))
        && cli.get(idx).is_some_and(|v| !v.starts_with("--"))
}

/// Pid file next to the DB, written by the running GUI window.
fn gui_pid_path(db_path: &std::path::Path) -> PathBuf {
    db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("gui.pid")
}

/// Whether the pid file points at a live process. A crash leaves the file
/// behind, so a dead pid (or garbage) counts as not running. Linux checks
/// `/proc`; elsewhere there is no cheap check, so assume not running —
/// matches the old behaviour of always booting a window there.
#[cfg(target_os = "linux")]
fn gui_is_alive(db_path: &std::path::Path) -> bool {
    let Ok(pid) = std::fs::read_to_string(gui_pid_path(db_path)) else {
        return false;
    };
    let Ok(pid) = pid.trim().parse::<u32>() else {
        return false;
    };
    pid > 0 && std::path::Path::new(&format!("/proc/{pid}")).exists()
}

/// No cheap liveness check off Linux: always boot, as before.
#[cfg(not(target_os = "linux"))]
fn gui_is_alive(_db_path: &std::path::Path) -> bool {
    false
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
        eprintln!("usage: mailapp [--status | --sync-once [--account <id|email>] | --open <id|email> [folder]] [--json]");
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

    // Sync mode takes the cross-process lock; a live holder (another
    // `--sync-once` still running) means the cache is fresh anyway, so report
    // it as locked. The GUI never holds it — outbox rows are claimed
    // atomically, so overlapping with an open GUI cannot double-send.
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

    // Resolve --account once, up front: an unknown value is a usage error
    // (exit 2) before any network or DB writes, and the id scopes both the
    // sync and the recent list so badge and popup agree with each other.
    let filter_account: Option<mailcore::models::Account> = match &filter {
        Some(f) => {
            let found = mailcore::store::accounts::list(&db)
                .unwrap_or_default()
                .into_iter()
                .find(|a| a.id.to_string() == *f || a.email_address == *f);
            match found {
                Some(a) => Some(a),
                None => {
                    eprintln!("no account matching '{f}'");
                    return 2;
                }
            }
        }
        None => None,
    };
    let filter_id = filter_account.as_ref().map(|a| a.id);

    let mut report = if sync && !locked {
        match filter_account {
            Some(a) => {
                let mut imap = mailcore::sync::imap::ImapSync::new(&a);
                let mut r = headless::SyncAllReport::default();
                let rt_res = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string());
                match rt_res.and_then(|rt| {
                    let s = rt
                        .block_on(mailcore::auth::load_account_secrets_retry(
                            &a.auth_vault_key,
                        ))
                        .map_err(|e| e.to_string())?;
                    rt.block_on(async {
                        imap.connect(&s.imap_password)
                            .await
                            .map_err(|e| e.to_string())?;
                        let ar = headless::sync_account(&db, &a, &mut imap, None).await;
                        imap.logout().await;
                        Ok((s, ar))
                    })
                }) {
                    Ok((_, ar)) => {
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
            None => headless::sync_all_accounts_blocking(&db),
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
    let recent = headless::recent_unread(&db, 10, filter_id);
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
