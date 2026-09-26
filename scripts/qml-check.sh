#!/usr/bin/env bash
# QML quality gate (see AGENT.md Definition of Done):
#   1. qmllint over all QML, failing on type errors and on unknown members
#      of Qt framework types (that class fails silently at runtime -- it hid
#      the dead WebEngine navigation guard).
#   2. Headless qmltestrunner unit tests (offscreen, stub Mailclient module
#      generated from the real singleton sources so nothing drifts).
set -euo pipefail
cd "$(dirname "$0")/.."

QT_BIN="${QT_BIN_DIR:-/usr/lib/qt6/bin}"
QMLLINT="$QT_BIN/qmllint"
RUNNER="$QT_BIN/qmltestrunner"
[ -x "$QMLLINT" ] || { echo "qmllint not found at $QMLLINT" >&2; exit 1; }
[ -x "$RUNNER" ] || { echo "qmltestrunner not found at $RUNNER" >&2; exit 1; }

echo "==> stub Mailclient module (from the real singleton sources)"
stub="$(mktemp -d)"
trap 'rm -rf "$stub"' EXIT
mkdir -p "$stub/Mailclient"
{
    echo "module Mailclient"
    for f in crates/mailapp/qml/*.qml; do
        base="$(basename "$f" .qml)"
        if head -1 "$f" | grep -q '^pragma Singleton'; then
            echo "singleton $base 1.0 $base.qml"
            cp "$f" "$stub/Mailclient/"
        fi
    done
} > "$stub/Mailclient/qmldir"
export QML_IMPORT_PATH="$stub"

echo "==> qmllint"
lint_out="$("$QMLLINT" -E crates/mailapp/qml/*.qml crates/mailapp/qml/components/*.qml crates/mailapp/qml/tests/*.qml 2>&1)" || true
fail=0
if printf '%s\n' "$lint_out" | grep -qE '\[(uncreatable-type|unresolved-type|ambiguous)\]'; then
    echo "qmllint: type errors (uncreatable/unresolved/ambiguous)" >&2
    printf '%s\n' "$lint_out" | grep -E '\[(uncreatable-type|unresolved-type|ambiguous)\]' >&2
    fail=1
fi
# Unknown-member reads/writes on Qt types fail silently at runtime (a call
# like `item.loadHtml(` would throw loudly instead, so calls are excluded):
# a missing method surfaces on first run, a missing enum never does.
qt_unknowns="$(printf '%s\n' "$lint_out" | awk '
/not found on type "Q[A-Z]/ {
    line = $0
    member = line
    sub(/.*Member "/, "", member)
    sub(/".*/, "", member)
    if ((getline nxt) > 0) {
        if (nxt !~ "\\." member "\\(") {
            print line
            print nxt
        }
    } else {
        print line
    }
}')"
if [ -n "$qt_unknowns" ]; then
    echo "qmllint: unknown member on a Qt framework type (silent no-op at runtime)" >&2
    printf '%s\n' "$qt_unknowns" >&2
    fail=1
fi
[ "$fail" = 0 ] || exit 1
echo "lint ok"

echo "==> qmltestrunner (offscreen)"
QT_QPA_PLATFORM=offscreen "$RUNNER" -input crates/mailapp/qml/tests
echo "qml tests ok"
