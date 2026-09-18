#!/usr/bin/env bash
# Install the mailclient.unread bar widget into the Omarchy shell:
# copies resources/omarchy/mailclient to ~/.config/omarchy/plugins,
# rescans, enables, and places it on the right side of the bar.
set -euo pipefail
cd "$(dirname "$0")/../.."

SRC="$PWD/resources/omarchy/mailclient"
DEST="$HOME/.config/omarchy/plugins/mailclient.unread"
mkdir -p "$HOME/.config/omarchy/plugins"
rm -rf "$DEST"
cp -r "$SRC" "$DEST"

omarchy-shell shell rescanPlugins
# The shell picks up new plugins via a file watcher; give it a moment
# before enabling (otherwise `enable` reports "plugin not known").
sleep 3
omarchy plugin enable mailclient.unread || {
    sleep 3
    omarchy plugin enable mailclient.unread
}
omarchy bar move mailclient.unread --section right

cat <<'EOF'
Installed. The widget syncs `mailapp --sync-once --json` every 15min
(configurable), notifies on new mail, and re-reads the cache on popup open:
    omarchy bar set mailclient.unread syncIntervalMin 5
    omarchy bar set mailclient.unread notify false
Requires `mailapp` on PATH (./scripts/install-local.sh).
EOF
