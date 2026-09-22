//! Startup and shutdown.

use flutter_rust_bridge::frb;

/// What the app needs to know about the core it just loaded.
#[frb]
#[derive(Clone, Debug)]
pub struct AppInfo {
    /// Absolute path of the SQLite file in use — shown in the About view and
    /// the first thing worth checking when "my mail is gone".
    pub db_path: String,
    /// `mailffi`'s version, which tracks the workspace's.
    pub version: String,
    pub license: String,
}

/// Called by flutter_rust_bridge once, before any other function.
#[frb(init)]
pub fn init_frb() {
    flutter_rust_bridge::setup_default_user_utils();
}

/// Open the database, run migrations, and start logging.
///
/// `data_dir` overrides where `mailclient.sqlite` lives. Desktop passes
/// `None` and gets `mailcore`'s platform path — deliberately the same file
/// the Qt app uses, so installing both does not split the mailbox in two.
/// Android has no XDG data dir, so the Dart side passes its
/// `getApplicationSupportDirectory()`.
///
/// Safe to call more than once (Flutter hot restart does): the second call
/// re-reads and returns the same answer. It fails only if `data_dir` tries to
/// move the database after it has already been opened.
pub fn init_app(data_dir: Option<String>) -> anyhow::Result<AppInfo> {
    init_logging();
    if let Some(dir) = data_dir {
        let dir = std::path::PathBuf::from(dir);
        // Re-setting the same directory is what a hot restart does; only a
        // genuine move is an error.
        if crate::db::db_path().parent() != Some(dir.as_path()) {
            crate::db::set_db_dir(dir)?;
        }
    }
    // Opening runs the migrations, so a failure here is the one worth
    // surfacing before the UI paints anything.
    let db = crate::db::shared_db()?;
    let _ = db;
    Ok(AppInfo {
        db_path: crate::db::db_path().to_string_lossy().into_owned(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        license: env!("CARGO_PKG_LICENSE").to_string(),
    })
}

/// Drop every pooled IMAP session. Call when the app goes away for good.
///
/// On desktop that is window close. On Android there is no reliable "app is
/// quitting" callback, so this belongs on `AppLifecycleState.detached` —
/// missing it costs nothing worse than a server-side session timing out.
pub fn shutdown() {
    crate::session::drop_all_sessions();
}

fn init_logging() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        #[cfg(target_os = "android")]
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("mailclient"),
        );
        #[cfg(not(target_os = "android"))]
        {
            // Same knob as mailapp: RUST_LOG tunes it, default is quiet.
            let _ =
                env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
                    .try_init();
        }
    });
}
