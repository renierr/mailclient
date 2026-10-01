#!/usr/bin/env bash
# The one way to format QML, so Linux and Windows produce the same bytes.
#
# qmlformat's output changes between Qt minor versions (e.g. how it indents
# arrow-function handler bodies), and .qmlformat.ini cannot pin that. So the
# version is pinned here: formatting with any other minor version is refused
# instead of silently reflowing lines formatted on the other machine. Bump
# PINNED deliberately (and reformat everything once) when the system Qt moves.
#
# Usage: scripts/qml-format.sh [--check] [file.qml ...]   (default: all QML)
#   --check  report files that are not formatted, change nothing (exit 1)
#
# Finds qmlformat via QMLFORMAT (any binary), else QT_BIN_DIR, else the
# Omarchy system Qt in /usr/lib/qt6/bin.
set -euo pipefail
cd "$(dirname "$0")/.."

PINNED="6.11"

check=0
if [ "${1:-}" = "--check" ]; then
    check=1
    shift
fi

if [ -n "${QMLFORMAT:-}" ]; then
    fmt="$QMLFORMAT"
else
    qt_bin="${QT_BIN_DIR:-/usr/lib/qt6/bin}"
    fmt="$qt_bin/qmlformat"
    [ -x "$fmt" ] || fmt="$qt_bin/qmlformat.exe"
fi
[ -x "$fmt" ] || { echo "qmlformat not found ($fmt); set QMLFORMAT or QT_BIN_DIR" >&2; exit 2; }

version="$("$fmt" --version | awk '{print $NF}')"
if [ "${version%.*}" != "$PINNED" ]; then
    echo "qmlformat $version, but QML is formatted with Qt $PINNED.x (see PINNED in $0)." >&2
    echo "Formatting with another version reflows code formatted on the other machine." >&2
    exit 2
fi

if [ "$#" -gt 0 ]; then
    files=("$@")
else
    mapfile -t files < <(git ls-files '*.qml')
fi

if [ "$check" = 1 ]; then
    bad=0
    for f in "${files[@]}"; do
        # Windows qmlformat writes CRLF to stdout whatever NewlineType says.
        if ! diff -q <(tr -d '\r' < "$f") <("$fmt" "$f" | tr -d '\r') > /dev/null; then
            echo "not formatted: $f"
            bad=1
        fi
    done
    exit "$bad"
fi

"$fmt" -i "${files[@]}"
