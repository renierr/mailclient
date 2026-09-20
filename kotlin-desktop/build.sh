#!/usr/bin/env bash
# Build in-process JNI backend (mailjni) + the Kotlin desktop app.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

if [ -z "${JAVA_HOME:-}" ] && [ -d /opt/android-studio/jbr ]; then
  export JAVA_HOME=/opt/android-studio/jbr
  export PATH="$JAVA_HOME/bin:$PATH"
fi

echo "==> cargo build -p mailjni --release"
cargo build --manifest-path "$ROOT/Cargo.toml" -p mailjni --release

echo "==> gradle :composeApp:build"
exec "$HERE/gradlew" -p "$HERE" :composeApp:build
