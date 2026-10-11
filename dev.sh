#!/usr/bin/env bash
# Dev loop: run the Qt/QML frontend (default), the egui frontend, or Flutter.
#
# Usage: ./dev.sh [--qt|--egui|--flutter] [-- extra args...]
#
# All frontends open the same local dev database (./data/dev.sqlite) so no
# dev run ever touches the real mailbox in ~/.local/share. Override with
# MAILCLIENT_DB for a throwaway file; the egui and Flutter apps read the same
# variable because their Rust core runs in-process.
# Works on Linux and in MSYS2/Git Bash on Windows (Qt path, see scripts/qt-env.sh).
set -euo pipefail
cd "$(dirname "$0")"

frontend="--qt"
case "${1:-}" in
    --qt | --flutter | --egui) frontend="$1"; shift ;;
esac

# Local dev database, shared by both frontends. Absolute: the Flutter path
# below cds into flutter/ before launching.
mkdir -p data
export MAILCLIENT_DB="${MAILCLIENT_DB:-$PWD/data/dev.sqlite}"
export RUST_LOG="${RUST_LOG:-info}"

# Load test credentials for local sync experiments (M1+). .env is gitignored.
if [ -f .env ]; then
    set -a
    # shellcheck disable=SC1091
    . ./.env
    set +a
fi

if [ "$frontend" = "--egui" ]; then
    echo "==> cargo run -p maildesk (MAILCLIENT_DB=$MAILCLIENT_DB)"
    exec cargo run -p maildesk -- "$@"
fi

if [ "$frontend" = "--flutter" ]; then
    echo "==> flutter run -d linux (MAILCLIENT_DB=$MAILCLIENT_DB)"
    cd flutter
    exec flutter run -d linux "$@"
fi

# shellcheck source=scripts/qt-env.sh
. ./scripts/qt-env.sh
# mailapp is a native Windows process under MSYS2, so it cannot resolve an
# MSYS-style path -- hand it a drive-letter one.
qml_dir="$PWD/crates/mailapp/qml"
[ -n "$EXE_SUFFIX" ] && qml_dir="$(cygpath -m "$qml_dir")"
export MAILCLIENT_QML_DIR="${MAILCLIENT_QML_DIR:-$qml_dir}"

exec cargo run -p mailapp -- "$@"
