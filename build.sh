#!/usr/bin/env bash
# Release build + dist bundle assembly.
#
# Usage: ./build.sh [--qt|--flutter|--all]  (default --qt)
#
# --qt:      cargo build --release -p mailapp
#            Output: dist/mailclient/{bin/mailapp,qml/,resources/,VERSION}
# --flutter: flutter build linux --release (the Rust core builds as part of it)
#            Output: dist/mailclient-flutter/{mailclient,lib/,data/,VERSION}
# Works on Linux and in MSYS2/Git Bash on Windows (Qt path, see scripts/qt-env.sh).
set -euo pipefail
cd "$(dirname "$0")"

target="${1:- --qt}"
case "$target" in
    --qt | --flutter | --all) ;;
    *) echo "usage: $0 [--qt|--flutter|--all]" >&2; exit 1 ;;
esac

write_version() {
    git rev-parse --short HEAD 2>/dev/null > "$1/VERSION" \
        || echo "unversioned" > "$1/VERSION"
}

build_qt() {
    # shellcheck source=scripts/qt-env.sh
    . ./scripts/qt-env.sh

    echo "==> cargo build --release -p mailapp"
    cargo build --release -p mailapp

    echo "==> assembling dist/mailclient"
    # A running instance keeps its Qt/Chromium files open, so this rm fails and
    # `set -e` would abort with a bare "Device or resource busy" after the bundle
    # was already half-deleted. Say what to do about it instead.
    if ! rm -rf dist/mailclient 2>/dev/null; then
        echo "cannot clear dist/mailclient -- files are in use." >&2
        echo "Close the running mailapp (its WebEngine process holds"          "resources/icudtl.dat) and run this again." >&2
        exit 1
    fi
    mkdir -p dist/mailclient/bin dist/mailclient/qml dist/mailclient/resources
    cp "target/release/mailapp$EXE_SUFFIX" dist/mailclient/bin/
    cp -r crates/mailapp/qml/* dist/mailclient/qml/
    cp -r resources/* dist/mailclient/resources/ 2>/dev/null || true
    write_version dist/mailclient

    # Unlike Linux (system Qt on the loader path), a Windows binary needs its Qt
    # DLLs, QML plugins and the WebEngine helper process copied in beside it.
    if [ -n "$EXE_SUFFIX" ]; then
        if [ -x "$QT_BIN_DIR/windeployqt.exe" ]; then
            echo "==> windeployqt"
            "$QT_BIN_DIR/windeployqt.exe" --release --qmldir crates/mailapp/qml \
                dist/mailclient/bin/mailapp.exe
        else
            echo "windeployqt not found next to qmake; dist/mailclient/bin will" \
                 "only run with Qt's bin/ on PATH." >&2
        fi
    fi

    cat <<EOF
Done. Run it with:
    ./dist/mailclient/bin/mailapp$EXE_SUFFIX
QML is embedded, with dist/mailclient/qml as filesystem fallback
(override: MAILCLIENT_QML_DIR=crates/mailapp/qml).
EOF

    if [ -z "$EXE_SUFFIX" ]; then
        cat <<'EOF'
Install to ~/.local:
    ./scripts/install-local.sh
EOF
    fi
}

build_flutter() {
    echo "==> flutter build linux --release"
    (cd flutter && flutter build linux --release)

    echo "==> assembling dist/mailclient-flutter"
    bundle="flutter/build/linux/x64/release/bundle"
    [ -d "$bundle" ] || { echo "expected bundle at $bundle -- build failed?" >&2; exit 1; }
    if ! rm -rf dist/mailclient-flutter 2>/dev/null; then
        echo "cannot clear dist/mailclient-flutter -- files are in use." >&2
        echo "Close the running mailclient and run this again." >&2
        exit 1
    fi
    mkdir -p dist/mailclient-flutter
    cp -r "$bundle"/* dist/mailclient-flutter/
    write_version dist/mailclient-flutter

    cat <<'EOF'
Done. Run it with:
    ./dist/mailclient-flutter/mailclient
Bundle layout (lib/libmailffi.so is the Rust core, loaded in-process).
EOF
}

case "$target" in
    --qt) build_qt ;;
    --flutter) build_flutter ;;
    --all) build_qt; build_flutter ;;
esac
