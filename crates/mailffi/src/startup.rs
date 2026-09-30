//! Process setup shared by both ways into this library: Dart's
//! [`crate::api::init::init_app`] and, on Android, the Kotlin entry points in
//! `android.rs`, which run without a Flutter engine.

/// Start logging once per process.
pub(crate) fn init_logging() {
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

/// Keep the database (and on Android the secrets vault) in `dir`.
///
/// Setting the same directory again is what a hot restart does, and what
/// the second entry point does when Dart got there first; only a genuine
/// move is an error.
pub(crate) fn use_data_dir(dir: std::path::PathBuf) -> anyhow::Result<()> {
    #[cfg(target_os = "android")]
    mailcore::auth::set_vault_dir(dir.clone());
    if crate::db::db_path().parent() != Some(dir.as_path()) {
        crate::db::set_db_dir(dir)?;
    }
    Ok(())
}
