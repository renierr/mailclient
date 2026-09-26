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

show_usage() {
    cat <<'EOF'
mailclient release build & packaging script

Usage: ./build.sh <target>

Available targets:
  --qt, --qml                  Build Qt/QML release bundle
                               Output: dist/mailclient/
  --flutter, --flutter-linux   Build Flutter Linux desktop release bundle
                               Output: dist/mailclient-flutter/
  --apk, --flutter-apk         Build signed Flutter Android APK
                               Output: dist/mailclient-apk/mailclient-release.apk
  --aab, --bundle              Build signed Flutter Android App Bundle (AAB)
                               Output: dist/mailclient-aab/mailclient-release.aab
  --all                        Build all desktop targets (Qt + Flutter Linux)
  -h, --help                   Show this help message

Examples:
  ./build.sh --qt
  ./build.sh --flutter
  ./build.sh --apk
  ./build.sh --aab
EOF
}

if [ $# -eq 0 ] || [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    show_usage
    exit 0
fi

target="$1"

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

build_apk() {
    echo "==> flutter build apk --release"
    (cd flutter && flutter build apk --release)

    echo "==> assembling dist/mailclient-apk"
    apk="flutter/build/app/outputs/flutter-apk/app-release.apk"
    [ -f "$apk" ] || { echo "expected apk at $apk -- build failed?" >&2; exit 1; }
    if ! rm -rf dist/mailclient-apk 2>/dev/null; then
        echo "cannot clear dist/mailclient-apk -- files are in use." >&2
        exit 1
    fi
    mkdir -p dist/mailclient-apk
    cp "$apk" dist/mailclient-apk/mailclient-release.apk
    write_version dist/mailclient-apk

    cat <<'EOF'
Done. APK available at:
    ./dist/mailclient-apk/mailclient-release.apk
Signed with the release keystore from flutter/android/key.properties.
EOF
}

build_aab() {
    echo "==> flutter build appbundle --release"
    (cd flutter && flutter build appbundle --release)

    echo "==> assembling dist/mailclient-aab"
    aab="flutter/build/app/outputs/bundle/release/app-release.aab"
    [ -f "$aab" ] || { echo "expected aab at $aab -- build failed?" >&2; exit 1; }
    if ! rm -rf dist/mailclient-aab 2>/dev/null; then
        echo "cannot clear dist/mailclient-aab -- files are in use." >&2
        exit 1
    fi
    mkdir -p dist/mailclient-aab
    cp "$aab" dist/mailclient-aab/mailclient-release.aab
    write_version dist/mailclient-aab

    cat <<'EOF'
Done. AAB bundle available at:
    ./dist/mailclient-aab/mailclient-release.aab
Signed with the release keystore from flutter/android/key.properties.
Ready for upload to Google Play Console.
EOF
}

case "$target" in
    --qt | --qml) build_qt ;;
    --flutter | --flutter-linux) build_flutter ;;
    --apk | --flutter-apk) build_apk ;;
    --aab | --flutter-aab | --bundle) build_aab ;;
    --all) build_qt; build_flutter ;;
    *)
        echo "Error: Unknown option '$target'" >&2
        echo "" >&2
        show_usage >&2
        exit 1
        ;;
esac
