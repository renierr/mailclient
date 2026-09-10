#!/usr/bin/env bash
# Build the release bundle and install it to ~/.local:
#   ~/.local/bin/mailapp
#   ~/.local/share/mailclient/{qml,resources}
#   ~/.local/share/applications/mailclient.desktop
set -euo pipefail
cd "$(dirname "$0")/.."

# ~/.local + a .desktop entry are freedesktop concepts; on Windows the
# dist/ bundle from build.sh is the deliverable.
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        echo "install-local.sh is Linux-only; run ./build.sh and use"              "dist/mailclient/ directly." >&2
        exit 1
        ;;
esac

./build.sh

BIN="$HOME/.local/bin"
SHARE="$HOME/.local/share/mailclient"
APPS="$HOME/.local/share/applications"
mkdir -p "$BIN" "$SHARE" "$APPS"

cp dist/mailclient/bin/mailapp "$BIN/"
cp -r dist/mailclient/qml "$SHARE/"
cp -r dist/mailclient/resources "$SHARE/" 2>/dev/null || true

# Desktop entry with the real home path baked in.
sed "s|@HOME@|$HOME|g" resources/mailclient.desktop > "$APPS/mailclient.desktop"
update-desktop-database "$APPS" 2>/dev/null || true

echo "Installed. Launch with: mailapp  (ensure ~/.local/bin is on PATH)"
