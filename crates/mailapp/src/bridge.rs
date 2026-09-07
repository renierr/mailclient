//! cxx-qt bridge: QObjects implemented in Rust, exposed to QML.
//!
//! M0 exposes [`qobject::Bridge`] (app identity, DB path, account count).
//! List models (`AccountListModel`, `FolderTreeModel`, `MessageListModel`)
//! and the composer controller land in M1–M3.

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
}

use cxx_qt_lib::QString;

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
