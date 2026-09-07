//! mailapp — Qt/QML shell over `mailcore`.
//!
//! Boots a `QGuiApplication` + `QQmlApplicationEngine`, opens (and migrates)
//! the SQLite DB, then loads `Main.qml` from the filesystem so designers can
//! iterate without recompiling Rust:
//!
//! 1. `$MAILCLIENT_QML_DIR/Main.qml` (explicit override)
//! 2. `<exe>/../share/mailclient/qml/Main.qml` (installed: `~/.local/...`)
//! 3. `<exe>/../qml/Main.qml` (dist bundle: `dist/mailclient/...`)
//! 4. `qml/Main.qml` (workspace root, `./scripts/dev.sh`)

pub mod bridge;

use std::path::PathBuf;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};
use mailcore::{Db, default_db_path};

/// Locate `Main.qml` using the search order documented above.
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
    let cwd = PathBuf::from("qml/Main.qml");
    if cwd.is_file() {
        return Some(cwd);
    }
    None
}

fn main() {
    env_logger::init();

    // Open (creating + migrating) the local cache DB before UI startup so
    // first paint already knows the DB path; models attach in M1.
    let db_path = default_db_path();
    match Db::open(&db_path) {
        Ok(_) => log::info!("opened database at {}", db_path.display()),
        Err(e) => log::error!("cannot open database at {}: {e}", db_path.display()),
    }

    let qml = find_main_qml().unwrap_or_else(|| {
        eprintln!("mailapp: Main.qml not found; set MAILCLIENT_QML_DIR");
        std::process::exit(1);
    });
    log::info!("loading QML from {}", qml.display());
    let abs = std::path::absolute(&qml).unwrap_or(qml);
    let url = QUrl::from(format!("file://{}", abs.display()).as_str());

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&url);
    }
    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
