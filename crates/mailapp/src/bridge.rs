//! cxx-qt bridge: QObjects implemented in Rust, exposed to QML.
//!
//! - [`qobject::Bridge`]: app identity, DB path, account count (M0).
//! - [`qobject::SettingsBridge`]: user preferences backed by the
//!   `mailcore` settings store, editable from the Settings dialog.
//! List models (`AccountListModel`, `FolderTreeModel`, `MessageListModel`)
//! land with the rest of M1.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        /// Qt QString.
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        /// App-level controller object.
        #[qobject]
        #[qproperty(QString, db_path)]
        #[qproperty(i32, account_count)]
        #[namespace = "mailclient"]
        type Bridge = super::BridgeRust;

        /// Health check callable from QML: returns `"pong: <message>"`.
        #[qinvokable]
        fn ping(&self, message: &QString) -> QString;
    }

    extern "RustQt" {
        /// User preferences, persisted in SQLite via `mailcore`.
        #[qobject]
        #[qml_element]
        #[qproperty(bool, sent_copy_enabled)]
        #[qproperty(bool, load_remote_images)]
        #[namespace = "mailclient"]
        type SettingsBridge = super::SettingsBridgeRust;

        /// Reload properties from the settings store.
        #[qinvokable]
        fn load(self: Pin<&mut Self>);

        /// Persist current properties to the settings store.
        #[qinvokable]
        fn save(self: Pin<&mut Self>);
    }
}

use core::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::store::settings;

/// Backing Rust struct for the `Bridge` QObject.
#[derive(Default)]
pub struct BridgeRust {
    db_path: QString,
    account_count: i32,
}

impl qobject::Bridge {
    /// Health check callable from QML.
    pub fn ping(&self, message: &QString) -> QString {
        let text = message.to_string();
        QString::from(format!("pong: {text}").as_str())
    }
}

/// Backing Rust struct for the `SettingsBridge` QObject.
pub struct SettingsBridgeRust {
    sent_copy_enabled: bool,
    load_remote_images: bool,
}

impl Default for SettingsBridgeRust {
    fn default() -> Self {
        Self {
            sent_copy_enabled: true,
            load_remote_images: false,
        }
    }
}

impl qobject::SettingsBridge {
    fn open_db() -> Option<mailcore::Db> {
        mailcore::Db::open(&mailcore::default_db_path())
            .map_err(|e| {
                log::warn!("settings: cannot open db: {e}");
            })
            .ok()
    }

    /// Reload properties from the settings store.
    pub fn load(mut self: Pin<&mut Self>) {
        if let Some(db) = Self::open_db() {
            self.as_mut().set_sent_copy_enabled(
                settings::get_bool(&db, settings::SENT_COPY_ENABLED).unwrap_or(true),
            );
            self.as_mut().set_load_remote_images(
                settings::get_bool(&db, settings::LOAD_REMOTE_IMAGES).unwrap_or(false),
            );
        }
    }

    /// Persist current properties to the settings store.
    pub fn save(self: Pin<&mut Self>) {
        if let Some(db) = Self::open_db() {
            let sent = *self.sent_copy_enabled();
            let remote = *self.load_remote_images();
            if let Err(e) = settings::set_bool(&db, settings::SENT_COPY_ENABLED, sent) {
                log::warn!("settings: cannot save sent-copy: {e}");
            }
            if let Err(e) = settings::set_bool(&db, settings::LOAD_REMOTE_IMAGES, remote) {
                log::warn!("settings: cannot save remote-images: {e}");
            }
        }
    }
}
