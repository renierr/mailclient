#!/usr/bin/env bash
# Dev loop for the Kotlin desktop frontend (Compose Multiplatform).
# Uses the dev DB in data/ (via .env) and builds/runs mailfeed + kotlin-desktop.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
exec "$HERE/kotlin-desktop/run.sh" "$@"
