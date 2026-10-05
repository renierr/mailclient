#!/usr/bin/env bash
# Native Android dev loop: start the emulator (if needed), installDebug the
# Compose app, launch it. Works on Omarchy Linux and MSYS2/Git Bash on
# Windows with the same invocation.
#
# Usage: ./scripts/android-dev.sh [--avd NAME] [--serial ID] [--no-build]
#        [--no-emulator] [--no-launch] [--seed] [--log] [-h]
#
#   --avd NAME     emulator AVD to boot (default: see resolution below)
#   --serial ID    adb device to use when several are attached
#   --no-build     skip installDebug (just boot + launch)
#   --no-emulator  never boot anything, fail if no device is online
#   --no-launch    build/install only
#   --seed         copy the Flutter app's database + vault into the native
#                  app first (fresh installs have no accounts yet; the
#                  account-setup screen does not exist)
#   --log          tail logcat for the app after launching (blocking)
#
# Machine-specific config never lives in git. Resolution order, first hit wins:
#   SDK dir : ANDROID_SDK_ROOT / ANDROID_HOME env, then sdk.dir in
#             android/local.properties (gitignored), then ~/Android/Sdk.
#   AVD     : --avd, then ANDROID_AVD env, then avd.name in
#             android/local.properties, then the only AVD if exactly one exists.
#   JDK     : JAVA_HOME env, then an Android Studio JBR if one is installed,
#             then whatever java is on PATH (needs 17+ for AGP 9).
# Examples (persist per machine, don't commit):
#   export ANDROID_AVD="Pixel_4a"           # ~/.bashrc, both machines
#   echo "sdk.dir=/opt/android-sdk" >> android/local.properties
set -euo pipefail
cd "$(dirname "$0")/.."

APP_ID="de.renier.mailclient.native"
FLUTTER_ID="de.renier.mailclient"
ACTIVITY="$APP_ID/.MainActivity"

AVD="" SERIAL="" BUILD=1 BOOT=1 LAUNCH=1 SEED="" LOGS=""

usage() { sed -n '2,24p' "$0"; }

while [ $# -gt 0 ]; do
    case "$1" in
        --avd) AVD="${2:-}"; shift 2 ;;
        --serial) SERIAL="${2:-}"; shift 2 ;;
        --no-build) BUILD=0; shift ;;
        --no-emulator) BOOT=0; shift ;;
        --no-launch) LAUNCH=0; shift ;;
        --seed) SEED=1; shift ;;
        --log) LOGS=1; shift ;;
        -h | --help) usage; exit 0 ;;
        *) echo "Unknown option '$1'" >&2; usage >&2; exit 1 ;;
    esac
done

# -- SDK ---------------------------------------------------------------
# Strip CR: local.properties may carry CRLF line endings. Always succeeds:
# callers run under `set -o pipefail`, where a grep with no match would
# otherwise abort the script with no message.
prop() {
    [ -f android/local.properties ] || return 0
    grep -E "^$1=" android/local.properties 2>/dev/null | cut -d= -f2- | tr -d '\r' | tail -n 1 || true
}

SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$(prop sdk.dir)}}"
[ -n "$SDK" ] || SDK="$HOME/Android/Sdk"
[ -d "$SDK/platform-tools" ] || {
    echo "Android SDK not found at '$SDK'." >&2
    echo "Set ANDROID_SDK_ROOT (or sdk.dir in android/local.properties, gitignored)." >&2
    exit 1
}
ADB="$SDK/platform-tools/adb"
[ -x "$ADB" ] || ADB="$ADB.exe"
EMULATOR="$SDK/emulator/emulator"
[ -x "$EMULATOR" ] || EMULATOR="$EMULATOR.exe"

# -- JDK ---------------------------------------------------------------
if [ -z "${JAVA_HOME:-}" ]; then
    for jbr in \
        "/opt/android-studio/jbr" \
        "$HOME/.local/share/JetBrains/Toolbox/apps/android-studio/jbr" \
        "/c/Program Files/Android/Android Studio/jbr" \
        "/d/Program Files/Android/Android Studio/jbr"; do
        if [ -x "$jbr/bin/java" ] || [ -x "$jbr/bin/java.exe" ]; then
            export JAVA_HOME="$jbr"
            break
        fi
    done
fi

# -- AVD ---------------------------------------------------------------
[ -n "$AVD" ] || AVD="${ANDROID_AVD:-$(prop avd.name)}"
if [ -z "$AVD" ]; then
    mapfile -t avds < <("$EMULATOR" -list-avds 2>/dev/null | tr -d '\r')
    if [ "${#avds[@]}" -eq 1 ]; then
        AVD="${avds[0]}"
    else
        echo "No AVD selected and ${#avds[@]} exist." >&2
        printf '  %s\n' "${avds[@]}" >&2
        echo "Pass --avd NAME, set ANDROID_AVD, or add avd.name to android/local.properties." >&2
        exit 1
    fi
fi
echo "==> AVD: $AVD"

adb() {
    if [ -n "$SERIAL" ]; then
        "$ADB" -s "$SERIAL" "$@"
    else
        "$ADB" "$@"
    fi
}

booted() {
    adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r' | grep -q 1
}

# -- emulator ----------------------------------------------------------
if [ "$BOOT" = 1 ] && ! booted; then
    echo "==> starting emulator ($AVD)"
    log="${TMPDIR:-/tmp}/mailclient-emulator.log"
    nohup "$EMULATOR" -avd "$AVD" >"$log" 2>&1 & disown 2>/dev/null || true
    echo "    log: $log"
fi
if ! booted; then
    if [ "$BOOT" = 0 ]; then
        echo "No booted device and --no-emulator was given." >&2
        exit 1
    fi
    echo "==> waiting for boot (up to 5 min)"
    for _ in $(seq 1 60); do
        sleep 5
        booted && break
    done
    booted || { echo "Emulator did not boot in time." >&2; exit 1; }
fi
echo "==> device: $(adb shell getprop ro.product.model 2>/dev/null | tr -d '\r')"

# -- seed (Flutter app data into the native app) ------------------------
if [ -n "$SEED" ]; then
    adb shell pm list packages 2>/dev/null | tr -d '\r' | grep -q "package:$FLUTTER_ID" || {
        echo "Seed needs the Flutter app ($FLUTTER_ID) installed on this device." >&2
        exit 1
    }
    echo "==> seeding from $FLUTTER_ID (both apps force-stopped)"
    adb shell am force-stop "$FLUTTER_ID"
    adb shell am force-stop "$APP_ID"
    tmp="/data/local/tmp"
    adb shell "run-as $FLUTTER_ID cp files/mailclient.sqlite files/auth_vault.json $tmp/ && chmod 644 $tmp/mailclient.sqlite $tmp/auth_vault.json"
    host_tmp="${TMPDIR:-/tmp}/mailclient-seed"
    mkdir -p "$host_tmp"
    adb pull "$tmp/mailclient.sqlite" "$host_tmp/" >/dev/null
    adb pull "$tmp/auth_vault.json" "$host_tmp/" >/dev/null
    adb push "$host_tmp/mailclient.sqlite" "$tmp/" >/dev/null
    adb push "$host_tmp/auth_vault.json" "$tmp/" >/dev/null
    adb shell "run-as $APP_ID cp $tmp/mailclient.sqlite $tmp/auth_vault.json files/ && chmod 600 files/mailclient.sqlite files/auth_vault.json && rm -f $tmp/mailclient.sqlite $tmp/auth_vault.json"
    rm -f "$host_tmp/mailclient.sqlite" "$host_tmp/auth_vault.json"
    echo "    seeded: open Home, read the ids, use Open message."
fi

# -- build & install ----------------------------------------------------
if [ "$BUILD" = 1 ]; then
    echo "==> installDebug (Rust core builds as part of it)"
    if [ -f "android/gradle/wrapper/gradle-wrapper.jar" ] && [ -x android/gradlew ]; then
        (cd android && ./gradlew installDebug)
    elif [ -f "android/gradle/wrapper/gradle-wrapper.jar" ]; then
        (cd android && bash gradlew installDebug)
    elif command -v gradle >/dev/null; then
        (cd android && gradle installDebug)
    else
        echo "No Gradle: open android/ in Android Studio once (writes the wrapper jar) or install Gradle." >&2
        exit 1
    fi
fi

# -- launch --------------------------------------------------------------
if [ "$LAUNCH" = 1 ]; then
    echo "==> launching $APP_ID"
    adb shell am start -n "$ACTIVITY" >/dev/null
fi

if [ -n "$LOGS" ]; then
    pid="$(adb shell pidof "$APP_ID" 2>/dev/null | tr -d '\r')"
    if [ -n "$pid" ]; then
        echo "==> logcat ($APP_ID, Ctrl-C to stop)"
        adb logcat --pid="$pid"
    else
        echo "App is not running; skipping logcat." >&2
    fi
fi

echo "Done."
