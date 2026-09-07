#!/usr/bin/env bash
# Dev loop: debug build + run against ./qml (no install).
set -euo pipefail
cd "$(dirname "$0")/.."

if [ -z "${QMAKE:-}" ] && [ -x /usr/lib/qt6/bin/qmake ]; then
    export QMAKE=/usr/lib/qt6/bin/qmake
fi
export QT_VERSION_MAJOR="${QT_VERSION_MAJOR:-6}"
export MAILCLIENT_QML_DIR="${MAILCLIENT_QML_DIR:-$PWD/qml}"
export RUST_LOG="${RUST_LOG:-info}"

exec cargo run -p mailapp -- "$@"
