#!/usr/bin/env bash
# Install or update the mailclient.unread bar widget in the Omarchy shell:
# copies resources/omarchy/mailclient to ~/.config/omarchy/plugins,
# rescans, enables, and places it on the right side of the bar.
#
# Re-running on an existing install updates the files in place. Because the
# running shell keeps the already-compiled widget cached (a rescan alone
# does not recompile unchanged URLs), an update also restarts the shell so
# QML changes take effect. Pass --no-restart to skip that (e.g. you will
# relog or restart the shell yourself); --restart forces it on fresh installs.
set -euo pipefail
cd "$(dirname "$0")/../.."

RESTART="auto"
for arg in "$@"; do
    case "$arg" in
        --no-restart) RESTART="never" ;;
        --restart) RESTART="always" ;;
        -h|--help)
            echo "usage: install-plugin.sh [--restart | --no-restart]"
            exit 0
            ;;
        *)
            echo "unknown option '$arg': usage: install-plugin.sh [--restart | --no-restart]" >&2
            exit 1
            ;;
    esac
done

SRC="$PWD/resources/omarchy/mailclient"
DEST="$HOME/.config/omarchy/plugins/mailclient.unread"
UPDATING=0
[ -d "$DEST" ] && UPDATING=1
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

if [ "$RESTART" = "always" ] || { [ "$RESTART" = "auto" ] && [ "$UPDATING" -eq 1 ]; }; then
    echo "Restarting Omarchy shell so the updated widget takes effect..."
    omarchy restart shell
fi

cat <<'EOF'
Installed. The widget syncs `mailapp --sync-once --json` every 15min
(configurable), notifies on new mail, and re-reads the cache on popup open:
    omarchy bar set mailclient.unread syncIntervalMin 5
    omarchy bar set mailclient.unread notify false
Requires `mailapp` on PATH (./scripts/install-local.sh).
EOF
