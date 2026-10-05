#!/usr/bin/env bash
# Shared Gradle preparation for android/. Sourced (never executed) by
# scripts/android-dev.sh and build.sh, both of which run from the repo root:
#
#   # shellcheck source=scripts/gradle-env.sh
#   . ./scripts/gradle-env.sh
#   gradle_env_setup android || exit 1
#
# Two machine-specific gaps it closes, so a fresh clone builds without any
# manual setup beyond the SDK/NDK/JDK themselves:
#
# 1. SDK env for Gradle. The dev script resolves the SDK for adb on its own,
#    but Gradle only looks at ANDROID_SDK_ROOT / ANDROID_HOME and at sdk.dir
#    in local.properties. When neither env var points at an SDK, export the
#    resolved one (env, then local.properties, then ~/Android/Sdk) for the
#    Gradle child. Writes no files.
# 2. Missing wrapper. gradlew + the wrapper jar are gitignored on purpose, so
#    a fresh clone has neither. When either is missing, regenerate both with
#    the `wrapper` task of the already-cached distribution that matches
#    gradle-wrapper.properties (no download, no system Gradle needed). The
#    tracked properties file is restored afterwards: newer Gradle rewrites
#    it (and flips -all to -bin), which must not leak into git.
#
# JDK fallback (JAVA_HOME, else an Android Studio JBR, else PATH java) lives
# here too, so both callers agree. Needs 17+ for AGP 9.
gradle_env_setup() {
    local dir="${1:-android}"

    # -- JDK -----------------------------------------------------------
    if [ -z "${JAVA_HOME:-}" ]; then
        local jbr
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

    # -- SDK env (Gradle reads env or local.properties, nothing else) ----
    if [ -z "${ANDROID_SDK_ROOT:-}" ] && [ -z "${ANDROID_HOME:-}" ]; then
        local sdk=""
        if [ -f "$dir/local.properties" ]; then
            sdk="$(grep -E "^sdk.dir=" "$dir/local.properties" 2>/dev/null | cut -d= -f2- | tr -d '\r' | tail -n 1 || true)"
        fi
        [ -n "$sdk" ] || sdk="$HOME/Android/Sdk"
        if [ -d "$sdk/platform-tools" ]; then
            export ANDROID_SDK_ROOT="$sdk" ANDROID_HOME="$sdk"
        else
            echo "Android SDK not found at '$sdk'." >&2
            echo "Set ANDROID_SDK_ROOT (or sdk.dir in $dir/local.properties, gitignored)." >&2
            return 1
        fi
    fi

    # -- wrapper (gitignored, so a fresh clone lacks it) -----------------
    if [ -f "$dir/gradle/wrapper/gradle-wrapper.jar" ] && [ -f "$dir/gradlew" ]; then
        return 0
    fi
    local props="$dir/gradle/wrapper/gradle-wrapper.properties"
    [ -f "$props" ] || {
        echo "No wrapper properties at '$props' -- cannot bootstrap Gradle." >&2
        return 1
    }
    # Strip CR: the properties may carry CRLF line endings.
    local url distver ver
    url="$(grep -E "^distributionUrl=" "$props" 2>/dev/null | cut -d= -f2- | tr -d '\r' | tail -n 1 || true)"
    distver="$(basename "$url" .zip)"
    ver="$(printf '%s' "$distver" | sed -E 's/^gradle-(.*)-(bin|all)$/\1/')"
    if [ -z "$distver" ] || [ "$ver" = "$distver" ]; then
        echo "Cannot parse Gradle version from '$props'." >&2
        return 1
    fi
    local gu="${GRADLE_USER_HOME:-$HOME/.gradle}"
    local gbin=""
    local cand
    # Quoted glob: expands to matches, or to itself (rejected by -x) when none.
    for cand in "$gu"/wrapper/dists/"$distver"/*/gradle-*/bin/gradle; do
        if [ -x "$cand" ]; then
            gbin="$cand"
            break
        fi
    done
    if [ -z "$gbin" ]; then
        echo "No Gradle wrapper in $dir/ and no cached $distver in $gu/wrapper/dists." >&2
        echo "Open $dir/ in Android Studio once (writes the wrapper jar) or install Gradle." >&2
        return 1
    fi
    echo "==> generating gradle wrapper with cached $distver (first run only)"
    local backup
    backup="$(mktemp)"
    cp "$props" "$backup"
    if ! (cd "$dir" && "$gbin" wrapper --gradle-version "$ver"); then
        cp "$backup" "$props"
        rm -f "$backup"
        return 1
    fi
    # The wrapper task reformats this tracked file; keep the repo's pin.
    cp "$backup" "$props"
    rm -f "$backup"
}
