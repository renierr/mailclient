#!/usr/bin/env bash
# Release build + dist bundle assembly.
# Output: dist/mailclient/{bin/mailapp,qml/,resources/,VERSION}
# Works on Linux and in MSYS2/Git Bash on Windows (see scripts/qt-env.sh).
set -euo pipefail
cd "$(dirname "$0")"

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
git rev-parse --short HEAD 2>/dev/null > dist/mailclient/VERSION \
    || echo "unversioned" > dist/mailclient/VERSION

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
