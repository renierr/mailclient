#!/usr/bin/env bash
# Dev loop: debug build + run against ./crates/mailapp/qml (no install).
# Works on Linux and in MSYS2/Git Bash on Windows (see scripts/qt-env.sh).
set -euo pipefail
cd "$(dirname "$0")"

# shellcheck source=scripts/qt-env.sh
. ./scripts/qt-env.sh
# mailapp is a native Windows process under MSYS2, so it cannot resolve an
# MSYS-style path -- hand it a drive-letter one.
qml_dir="$PWD/crates/mailapp/qml"
[ -n "$EXE_SUFFIX" ] && qml_dir="$(cygpath -m "$qml_dir")"
export MAILCLIENT_QML_DIR="${MAILCLIENT_QML_DIR:-$qml_dir}"
export RUST_LOG="${RUST_LOG:-info}"

# Load test credentials for local sync experiments (M1+). .env is gitignored.
if [ -f .env ]; then
    set -a
    # shellcheck disable=SC1091
    . ./.env
    set +a
fi

exec cargo run -p mailapp -- "$@"
