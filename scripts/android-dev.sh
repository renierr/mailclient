#!/usr/bin/env bash
# Native Android dev loop: start the emulator (if needed), installDebug the
# Compose app, launch it. Works on Omarchy Linux and MSYS2/Git Bash on
# Windows with the same invocation.
#
# Bare invocation prints the help below and runs nothing (same convention
# as build.sh: every run is explicit). Any flag starts the loop.
#
# Machine-specific config never lives in git. Resolution order, first hit wins:
#   SDK dir : ANDROID_SDK_ROOT / ANDROID_HOME env, then sdk.dir in
#             android/local.properties (gitignored; forward slashes even on
#             Windows — AGP 9 rejects backslash paths), then ~/Android/Sdk.
#   AVD     : --avd, then ANDROID_AVD env, then avd.name in
#             android/local.properties, then the only AVD if exactly one exists.
#   JDK     : JAVA_HOME env, then an Android Studio JBR if one is installed,
#             then whatever java is on PATH (needs 17+ for AGP 9).
# A missing Gradle wrapper (gitignored) is regenerated from the cached
# distribution matching gradle-wrapper.properties, and the resolved SDK is
# exported for the Gradle child — a fresh clone builds with no manual setup
# (see scripts/gradle-env.sh).
# Examples (persist per machine, don't commit):
#   export ANDROID_AVD="Pixel_4a"           # ~/.bashrc, both machines
#   echo "sdk.dir=/opt/android-sdk" >> android/local.properties
set -euo pipefail
cd "$(dirname "$0")/.."

# shellcheck source=scripts/gradle-env.sh
. ./scripts/gradle-env.sh

APP_ID="de.renier.mailclient.native"
# Fully qualified: the class lives in namespace de.renier.mailclient while
# the applicationId carries the .native suffix, so the .MainActivity
# shorthand would resolve to a class that does not exist.
ACTIVITY="$APP_ID/de.renier.mailclient.MainActivity"

AVD="" SERIAL="" ASSUME_YES=0
WANT_CLEAN="" WANT_BUILD="" WANT_DIST="" WANT_EMU="" WANT_RUN="" WANT_LOG="" WANT_UNINSTALL=""
WANT_SEED_DB="" SEED_SRC="data/dev.sqlite"

show_help() {
    cat <<'EOF'
mailclient native Android tasks: build, install, run, and inspect.

Usage: ./scripts/android-dev.sh --build|--dist|--run [TASK]... [OPTION]...

Tasks (several combine; they run in the order listed here):
  --clean         gradlew clean
  --build         debug build only (assembleDebug, no device needed)
  --dist          signed release APK into dist/ (needs android/key.properties)
  --emulator      boot the emulator and wait for it (asks first, see below)
  --run           install the debug build on the device and launch the app
  --uninstall     remove the app from the device (fresh reinstall: add --run)
  --seed-db [FILE] copy a desktop database (default: data/dev.sqlite,
                  gitignored, never committed) into the app as
                  mailclient.sqlite — dev-only fixture for offline reads;
                  secrets stay in the desktop keyring, so on-device
                  sync/send needs the account password (Step 2)
  --log           tail logcat for the app (it must be running; blocking)

Options:
  --avd NAME      which emulator AVD to boot
  --serial ID     which adb device to use when several are attached
  -y, --yes       answer "boot the emulator?" with yes (non-interactive use)
  -h, --help      show this help

Devices: --emulator, --run, --uninstall and --log need a booted device.
When none is online the script asks whether to boot the resolved AVD
(--yes answers yes; without a terminal it fails instead of asking).

Machine config (never committed, first hit wins):
  SDK dir : ANDROID_SDK_ROOT / ANDROID_HOME, then sdk.dir in
            android/local.properties (forward slashes even on Windows),
            then ~/Android/Sdk
  AVD     : --avd, then ANDROID_AVD, then avd.name in
            android/local.properties, then the only AVD if exactly one
  JDK     : JAVA_HOME, then an Android Studio JBR if one is installed,
            then whatever java is on PATH (needs 17+ for AGP 9)

Examples:
  ./scripts/android-dev.sh --run
  ./scripts/android-dev.sh --run --log
  ./scripts/android-dev.sh --emulator --avd Pixel_4a
  ./scripts/android-dev.sh --uninstall --run
  ./scripts/android-dev.sh --seed-db --run
  ./scripts/android-dev.sh --dist
EOF
}

[ $# -eq 0 ] && { show_help; exit 0; }

while [ $# -gt 0 ]; do
    case "$1" in
        --clean) WANT_CLEAN=1; shift ;;
        --build) WANT_BUILD=1; shift ;;
        --dist) WANT_DIST=1; shift ;;
        --emulator) WANT_EMU=1; shift ;;
        --run) WANT_RUN=1; shift ;;
        --uninstall) WANT_UNINSTALL=1; shift ;;
        --seed-db)
            WANT_SEED_DB=1
            if [ -n "${2:-}" ] && [ "${2:0:2}" != "--" ]; then
                SEED_SRC="$2"; shift 2
            else
                shift
            fi
            ;;
        --log) WANT_LOG=1; shift ;;
        --avd) AVD="${2:-}"; shift 2 ;;
        --serial) SERIAL="${2:-}"; shift 2 ;;
        -y | --yes) ASSUME_YES=1; shift ;;
        -h | --help) show_help; exit 0 ;;
        *) echo "Unknown option '$1'" >&2; show_help >&2; exit 1 ;;
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

# -- JDK + SDK env + wrapper: owned by scripts/gradle-env.sh, applied lazily
# in run_gradle (device-free tasks like --build need no device, but every
# Gradle task needs all three).

# -- AVD (resolved lazily: --build/--dist/--clean need no device) -----
resolve_avd() {
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
}

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

boot_emulator() {
    resolve_avd
    echo "==> starting emulator ($AVD)"
    log="${TMPDIR:-/tmp}/mailclient-emulator.log"
    nohup "$EMULATOR" -avd "$AVD" >"$log" 2>&1 & disown 2>/dev/null || true
    echo "    log: $log"
}

wait_for_boot() {
    echo "==> waiting for boot (up to 5 min)"
    for _ in $(seq 1 60); do
        sleep 5
        booted && break
    done
    booted || { echo "Emulator did not boot in time." >&2; exit 1; }
    echo "==> device: $(adb shell getprop ro.product.model 2>/dev/null | tr -d '\r')"
}

# Every device-needing task comes through here. An explicitly selected
# serial is never second-guessed: if that device is not booted, fail
# instead of booting a different one.
ensure_device() {
    if booted; then
        return 0
    fi
    if [ -n "$SERIAL" ]; then
        echo "Device '$SERIAL' is not booted." >&2
        exit 1
    fi
    local answer="y"
    if [ "$ASSUME_YES" = 0 ]; then
        if [ -t 0 ]; then
            resolve_avd
            printf "No emulator or device is online. Boot AVD '%s'? [Y/n] " "$AVD"
            IFS= read -r answer || answer=""
            [ -n "$answer" ] || answer="y"
        else
            echo "No device online. Re-run with --yes to boot one, or start it first." >&2
            exit 1
        fi
    fi
    case "$answer" in
        [Yy]*) boot_emulator; wait_for_boot ;;
        *) echo "Aborted: no device." >&2; exit 1 ;;
    esac
}

run_gradle() {
    gradle_env_setup android || exit 1
    if [ -f "android/gradle/wrapper/gradle-wrapper.jar" ] && [ -x android/gradlew ]; then
        (cd android && ./gradlew "$@")
    elif [ -f "android/gradle/wrapper/gradle-wrapper.jar" ]; then
        (cd android && bash gradlew "$@")
    elif command -v gradle >/dev/null; then
        (cd android && gradle "$@")
    else
        echo "No Gradle: open android/ in Android Studio once (writes the wrapper jar) or install Gradle." >&2
        exit 1
    fi
}

# -- tasks (fixed order: teardown, build, device, inspect) ---------------
if [ -n "$WANT_UNINSTALL" ]; then
    ensure_device
    echo "==> uninstalling $APP_ID"
    adb uninstall "$APP_ID"
fi

if [ -n "$WANT_CLEAN" ]; then
    echo "==> gradle clean"
    run_gradle clean
fi

if [ -n "$WANT_BUILD" ]; then
    echo "==> assembleDebug (Rust core builds as part of it)"
    run_gradle assembleDebug
fi

if [ -n "$WANT_DIST" ]; then
    ./build.sh --android
fi

if [ -n "$WANT_SEED_DB" ]; then
    [ -f "$SEED_SRC" ] || {
        echo "Seed database not found: $SEED_SRC" >&2
        echo "Pass a path: ./scripts/android-dev.sh --seed-db /path/to.db" >&2
        exit 1
    }
    ensure_device
    echo "==> seeding app DB from $SEED_SRC (dev-only fixture, never committed)"
    adb shell am force-stop "$APP_ID" >/dev/null
    if command -v sqlite3 >/dev/null; then
        sqlite3 "$SEED_SRC" "PRAGMA wal_checkpoint(TRUNCATE);" >/dev/null
    fi
    tmp="/data/local/tmp/mailclient-seed.sqlite"
    # MSYS2 rewrites leading-slash args to Windows paths, including this
    # remote one — exempt exactly this call from conversion.
    MSYS_NO_PATHCONV=1 adb push "$SEED_SRC" "$tmp" >/dev/null
    # Absolute package paths (run-as does not guarantee its working dir),
    # one command per call: `run-as pkg a && b` would run b as shell.
    datadir="/data/data/$APP_ID/files"
    as_app="run-as $APP_ID"
    if ! {
        adb shell "$as_app mkdir -p $datadir" >/dev/null &&
        adb shell "$as_app cp $tmp $datadir/mailclient.sqlite" >/dev/null &&
        adb shell "$as_app chmod 600 $datadir/mailclient.sqlite" >/dev/null &&
        adb shell "$as_app rm -f $datadir/mailclient.sqlite-wal $datadir/mailclient.sqlite-shm" >/dev/null &&
        adb shell "rm -f $tmp" >/dev/null
    }; then
        echo "Seed needs a debuggable build (run-as failed)." >&2
        exit 1
    fi
    src_bytes="$(wc -c <"$SEED_SRC" | tr -d ' ')"
    dst_bytes="$(adb shell "run-as $APP_ID stat -c %s $datadir/mailclient.sqlite" 2>/dev/null | tr -d '\r')"
    [ "$src_bytes" = "$dst_bytes" ] || {
        echo "Seed copy size mismatch: host $src_bytes vs device $dst_bytes." >&2
        exit 1
    }
    echo "    seeded ($src_bytes bytes): offline reads work; sync/send need the account password (PLAN.md Step 2)"
fi

if [ -n "$WANT_EMU" ]; then
    ensure_device
    echo "==> device: $(adb shell getprop ro.product.model 2>/dev/null | tr -d '\r')"
fi

if [ -n "$WANT_RUN" ]; then
    ensure_device
    echo "==> installDebug (Rust core builds as part of it)"
    run_gradle installDebug
    echo "==> launching $APP_ID"
    adb shell am start -n "$ACTIVITY" >/dev/null
fi

if [ -n "$WANT_LOG" ]; then
    ensure_device
    pid="$(adb shell pidof "$APP_ID" 2>/dev/null | tr -d '\r')"
    if [ -n "$pid" ]; then
        echo "==> logcat ($APP_ID, Ctrl-C to stop)"
        adb logcat --pid="$pid"
    else
        echo "App is not running on this device." >&2
        exit 1
    fi
fi

echo "Done."
