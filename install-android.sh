#!/usr/bin/env bash
# Install the built Android release APK on a phone or a running emulator.
#
# Usage: ./install-android.sh [--build] [--serial ID] [--launch]
#
#   --build    run ./build-android.sh first
#   --serial   which adb device to use when several are attached
#   --launch   start the app once it is installed
#
# Reinstalls over what is there (adb install -r), so accounts, settings and
# cached mail are kept. Never uninstalls: that would wipe them with no way
# back (AGENTS.md §2) — if the install is refused over a differently signed
# build, the message says so and the app stays as it is.
#
# SDK dir: ANDROID_SDK_ROOT / ANDROID_HOME env, then sdk.dir in
#          android/local.properties (gitignored), then ~/Android/Sdk.
set -euo pipefail
cd "$(dirname "$0")"

BUILD=0
LAUNCH=0
SERIAL=""
while [ $# -gt 0 ]; do
    case "$1" in
        --build) BUILD=1; shift ;;
        --serial) SERIAL="${2:?--serial needs an id}"; shift 2 ;;
        --launch) LAUNCH=1; shift ;;
        -h|--help) grep '^#' "$0" | tail -n +2 | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1 (try --help)" >&2; exit 2 ;;
    esac
done

if [ "$BUILD" = 1 ]; then
    ./build-android.sh
fi

APK_DIR="dist/mailclient-android"
APK="$(ls -t "$APK_DIR"/mailclient-native-*-release.apk 2>/dev/null | head -1 || true)"
if [ -z "$APK" ]; then
    echo "No release APK in $APK_DIR/." >&2
    echo "Build one first:  ./build-android.sh" >&2
    exit 1
fi

prop() {
    [ -f android/local.properties ] || return 0
    grep -E "^$1=" android/local.properties 2>/dev/null | cut -d= -f2- | tr -d '\r' | tail -n 1 || true
}
SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$(prop sdk.dir)}}"
[ -n "$SDK" ] || SDK="$HOME/Android/Sdk"
ADB="$SDK/platform-tools/adb"
[ -x "$ADB" ] || ADB="$ADB.exe"
[ -x "$ADB" ] || ADB="adb"

# Which device: --serial, else the only one online.
DEVICE_ARGS=()
if [ -n "$SERIAL" ]; then
    DEVICE_ARGS=(-s "$SERIAL")
else
    ONLINE="$("$ADB" devices | awk 'NR > 1 && $2 == "device" { print $1 }')"
    COUNT="$(printf '%s' "$ONLINE" | grep -c . || true)"
    if [ "$COUNT" != "1" ]; then
        echo "attached devices:" >&2
        "$ADB" devices -l >&2
        [ "$COUNT" = "0" ] && echo "" >&2
        echo "Pick one with --serial ID (or start the emulator)." >&2
        exit 1
    fi
fi

APP_ID="$(sed -n 's/.*applicationId = "\([^"]*\)".*/\1/p' android/app/build.gradle.kts | head -1)"
echo "==> installing $(basename "$APK")"
if ! "$ADB" ${DEVICE_ARGS[@]+"${DEVICE_ARGS[@]}"} install -r "$APK"; then
    echo "" >&2
    echo "Install refused: the build on the device was signed with another key." >&2
    echo "The app there is untouched — this never uninstalls, which would wipe" >&2
    echo "accounts, settings and cached mail (AGENTS.md §2)." >&2
    echo "For the debug dev loop on an emulator:  ./scripts/android-dev.sh --run" >&2
    exit 1
fi

MODEL="$("$ADB" ${DEVICE_ARGS[@]+"${DEVICE_ARGS[@]}"} shell getprop ro.product.model 2>/dev/null | tr -d '\r')"
echo "==> done on ${MODEL:-the device} ($APP_ID)"

if [ "$LAUNCH" = 1 ]; then
    "$ADB" ${DEVICE_ARGS[@]+"${DEVICE_ARGS[@]}"} shell am start -n "$APP_ID/de.renier.mailclient.MainActivity" >/dev/null
    echo "==> launched"
fi
