//! Build script: compile the cxx-qt bridge and register the QML module.
//!
//! Qt location: cxx-qt-build finds Qt via the `QMAKE` env var first, then
//! `qmake` on PATH -- and that is deliberately all this script knows. Locating
//! Qt is left to the wrapper scripts (`scripts/*.sh`, incl. Windows via
//! MSYS2/Git Bash), which export QMAKE plus `QT_VERSION_MAJOR=6`; there is no
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
//!
//! The app icon is a build-time concern on Windows only: the shell reads it
//! from a Win32 resource linked into the exe, and Qt's windows plugin reads
//! the same resource for the title-bar icon. Linux takes it from the
//! `.desktop` file instead, so there is nothing to embed there.

use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

/// Link `resources/mailclient.ico` into the exe.
///
/// Two resource names, because two consumers look for different ones:
/// `1` is the lowest-id icon, which is what Explorer and the taskbar show for
/// the executable, and `IDI_ICON1` is the literal name Qt's windows platform
/// plugin passes to `LoadImage` when it registers the window class -- without
/// it the window and its Alt-Tab entry fall back to the generic Qt icon.
///
/// A failure here is a warning, not an error: compiling the resource needs
/// `rc.exe` (Windows SDK) or `windres`, and a missing icon must not be the
/// reason a build fails.
#[cfg(windows)]
fn embed_icon() {
    let icon = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../resources/mailclient.ico"
    );
    println!("cargo:rerun-if-changed={icon}");
    let mut res = winresource::WindowsResource::new();
    res.set_icon_with_id(icon, "1")
        .set_icon_with_id(icon, "IDI_ICON1");
    if let Err(e) = res.compile() {
        println!("cargo:warning=app icon not embedded: {e}");
    }
}

#[cfg(not(windows))]
fn embed_icon() {}

fn main() {
    embed_icon();

    CxxQtBuilder::new_qml_module(
        QmlModule::new("Mailclient")
            // Design tokens, registered as a singleton so every pane reads
            // the same values via `import Mailclient` (see qml/Theme.qml).
            .qml_file(QmlFile::from("qml/Theme.qml").singleton(true))
            // In-place ListModel updates, shared by every pane that owns a
            // feed model (see qml/ModelSync.qml).
            .qml_file(QmlFile::from("qml/ModelSync.qml").singleton(true))
            // Vector icon codepoints for the bundled Material Icons font
            // (see qml/Icons.qml, qml/fonts/ATTRIBUTION.md).
            .qml_file(QmlFile::from("qml/Icons.qml").singleton(true))
            // One contract for parsing bridge payloads, so a malformed one
            // cannot abandon a reload half-way (see qml/FeedJson.qml).
            .qml_file(QmlFile::from("qml/FeedJson.qml").singleton(true))
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
            .qml_file("qml/components/AppMenuItem.qml")
            .qml_file("qml/components/RecipientField.qml")
            .qml_file("qml/components/EditorFrame.qml")
            .qml_file("qml/components/ComposerToolbar.qml")
            .qml_file("qml/components/ComposerAttachmentTray.qml")
            .qml_file("qml/components/BulkActionBar.qml")
            .qml_file("qml/components/AppDialog.qml"),
    )
    .file("src/bridge.rs")
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .qt_module("Network")
    .qt_module("WebEngineQuick")
    // The bundled icon font, embedded next to the QML that loads it:
    // `qrc:/qt/qml/Mailclient/qml/fonts/…`, so the FontLoader's relative
    // source resolves identically in embedded, dist and dev runs (the dist
    // copy mirrors qml/ one to one).
    .qrc_resources(["qml/fonts/MaterialIcons-Regular.ttf"])
    .build();
}
