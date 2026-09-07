//! Build script: compile the cxx-qt bridge and register the QML module.
//!
//! Qt location: cxx-qt-build finds Qt via the `QMAKE` env var first, then
//! `qmake` on PATH. On Omarchy/Arch, qmake lives at
//! `/usr/lib/qt6/bin/qmake` (not on PATH), so `scripts/*.sh` export QMAKE.
//! `QT_VERSION_MAJOR=6` is set there as well to disambiguate.
//!
//! QML lives in `crates/mailapp/qml/` (single source of truth) and is
//! embedded via the `Mailclient` QML module (`qrc:/qt/qml/Mailclient/...`).
//! At runtime `main.rs` prefers an explicit `$MAILCLIENT_QML_DIR` override
//! (designer iteration) and falls back to the embedded module.

use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Mailclient")
            .qml_file("qml/Main.qml")
            .qml_file("qml/Sidebar.qml")
            .qml_file("qml/MessageList.qml")
            .qml_file("qml/MessageView.qml")
            .qml_file("qml/Composer.qml")
            .qml_file("qml/AccountSetup.qml")
            .qml_file("qml/Settings.qml")
            .qml_file("qml/components/Avatar.qml"),
    )
    .file("src/bridge.rs")
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .qt_module("Network")
    .qt_module("WebEngineQuick")
    .build();
}
