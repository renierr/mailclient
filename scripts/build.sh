#!/usr/bin/env bash
# Release build + dist bundle assembly.
# Output: dist/mailclient/{bin/mailapp,qml/,resources/,VERSION}
set -euo pipefail
cd "$(dirname "$0")/.."

# Qt location on Omarchy/Arch (qmake is not on PATH by default).
if [ -z "${QMAKE:-}" ] && [ -x /usr/lib/qt6/bin/qmake ]; then
    export QMAKE=/usr/lib/qt6/bin/qmake
fi
export QT_VERSION_MAJOR="${QT_VERSION_MAJOR:-6}"

echo "==> cargo build --release -p mailapp (QMAKE=${QMAKE:-<path>})"
cargo build --release -p mailapp

echo "==> assembling dist/mailclient"
rm -rf dist/mailclient
mkdir -p dist/mailclient/bin dist/mailclient/qml dist/mailclient/resources
cp target/release/mailapp dist/mailclient/bin/
cp -r crates/mailapp/qml/* dist/mailclient/qml/
cp -r resources/* dist/mailclient/resources/ 2>/dev/null || true
git rev-parse --short HEAD 2>/dev/null > dist/mailclient/VERSION \
    || echo "unversioned" > dist/mailclient/VERSION

cat <<EOF
Done. Run it with:
    ./dist/mailclient/bin/mailapp
QML is embedded, with dist/mailclient/qml as filesystem fallback
(override: MAILCLIENT_QML_DIR=crates/mailapp/qml).
Install to ~/.local:
    ./scripts/install-local.sh
EOF
