#!/usr/bin/env bash
# Build the Qt-free backend (mailfeed) + the Kotlin desktop app.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

if [ -z "${JAVA_HOME:-}" ] && [ -d /opt/android-studio/jbr ]; then
  export JAVA_HOME=/opt/android-studio/jbr
  export PATH="$JAVA_HOME/bin:$PATH"
fi

echo "==> cargo build -p mailjni --release"
cargo build --manifest-path "$ROOT/Cargo.toml" -p mailjni --release

echo "==> cargo build -p mailfeed"
cargo build --manifest-path "$ROOT/Cargo.toml" -p mailfeed --release
export MAILFEED_BIN="${MAILFEED_BIN:-$ROOT/target/release/mailfeed}"

echo "==> gradle :composeApp:build (mailfeed: $MAILFEED_BIN)"
exec "$HERE/gradlew" -p "$HERE" :composeApp:build
