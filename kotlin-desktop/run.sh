#!/usr/bin/env bash
# Build backend + start the Kotlin desktop mail client (Compose Multiplatform).
# Honors MAILCLIENT_DB (same SQLite file as the QML app) and MAILFEED_BIN.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

# Load test credentials / dev DB from .env (same as ./dev.sh)
if [ -f "$ROOT/.env" ]; then
  set -a
  # shellcheck disable=SC1091
  . "$ROOT/.env"
  set +a
fi

if [ -z "${JAVA_HOME:-}" ] && [ -d /opt/android-studio/jbr ]; then
  export JAVA_HOME=/opt/android-studio/jbr
  export PATH="$JAVA_HOME/bin:$PATH"
fi

if [ -z "${MAILFEED_BIN:-}" ] || [ ! -x "${MAILFEED_BIN:-}" ]; then
  echo "==> cargo build -p mailfeed --release"
  cargo build --manifest-path "$ROOT/Cargo.toml" -p mailfeed --release
  export MAILFEED_BIN="$ROOT/target/release/mailfeed"
fi

echo "==> starting Kotlin desktop app (mailfeed: $MAILFEED_BIN)"
exec "$HERE/gradlew" -p "$HERE" :composeApp:run
