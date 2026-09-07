//! Build script: compile the cxx-qt bridge (src/bridge.rs) and link Qt.
//!
//! Qt location: cxx-qt-build finds Qt via the `QMAKE` env var first, then
//! `qmake` on PATH. On Omarchy/Arch, qmake lives at
//! `/usr/lib/qt6/bin/qmake` (not on PATH), so `scripts/*.sh` export QMAKE.
//! `QT_VERSION_MAJOR=6` is set there as well to disambiguate.

fn main() {
    cxx_qt_build::CxxQtBuilder::new()
        .file("src/bridge.rs")
        .qt_module("Qml")
        .qt_module("Quick")
        .qt_module("QuickControls2")
        .qt_module("Network")
        .build();
}
