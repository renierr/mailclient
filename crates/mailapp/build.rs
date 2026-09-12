//! Build script: compile the cxx-qt bridge and register the QML module.
//!
//! Qt location: cxx-qt-build finds Qt via the `QMAKE` env var first, then
//! `qmake` on PATH -- and that is deliberately all this script knows. Locating
//! Qt is left to the wrapper scripts (`scripts/*.sh` on Linux, `*.ps1` on
//! Windows), which export QMAKE plus `QT_VERSION_MAJOR=6`; there is no
//! OS-specific branching here, because a build script cannot install Qt and
//! a per-OS path list inside it would just duplicate that discovery.
//! On Arch/Omarchy qmake sits at `/usr/lib/qt6/bin/qmake` (off PATH); on
//! Windows it is under the Qt kit dir, e.g.
//! `C:/Qt/<version>/msvc2022_64/bin/qmake.exe`. The Qt kit ABI must match
//! the Rust host toolchain (MSVC Qt for `*-pc-windows-msvc`).
//!
//! QML lives in `crates/mailapp/qml/` (single source of truth) and is
//! embedded via the `Mailclient` QML module (`qrc:/qt/qml/Mailclient/...`).
//! At runtime `main.rs` prefers an explicit `$MAILCLIENT_QML_DIR` override
//! (designer iteration) and falls back to the embedded module.

use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("Mailclient")
            // Design tokens, registered as a singleton so every pane reads
            // the same values via `import Mailclient` (see qml/Theme.qml).
            .qml_file(QmlFile::from("qml/Theme.qml").singleton(true))
            // In-place ListModel updates, shared by every pane that owns a
            // feed model (see qml/ModelSync.qml).
            .qml_file(QmlFile::from("qml/ModelSync.qml").singleton(true))
            .qml_file("qml/Main.qml")
            .qml_file("qml/Sidebar.qml")
            .qml_file("qml/MessageList.qml")
            .qml_file("qml/MessageView.qml")
            .qml_file("qml/Composer.qml")
            .qml_file("qml/Contacts.qml")
            .qml_file("qml/AccountSetup.qml")
            .qml_file("qml/Accounts.qml")
            .qml_file("qml/Folders.qml")
            .qml_file("qml/MoveTo.qml")
            .qml_file("qml/Settings.qml")
            .qml_file("qml/components/Avatar.qml")
            .qml_file("qml/components/IconButton.qml")
            .qml_file("qml/components/FormField.qml")
            .qml_file("qml/components/AppTextField.qml")
            .qml_file("qml/components/AppButton.qml")
            .qml_file("qml/components/AppComboBox.qml")
            .qml_file("qml/components/AppCheckBox.qml")
            .qml_file("qml/components/AppMenu.qml")
            .qml_file("qml/components/RecipientField.qml")
            .qml_file("qml/components/EditorFrame.qml"),
    )
    .file("src/bridge.rs")
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .qt_module("Network")
    .qt_module("WebEngineQuick")
    .build();
}
