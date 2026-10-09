#!/usr/bin/env bash
# Build the native Android app as a signed release APK.
#
# Usage: ./build-android.sh   (any argument is passed on to ./build.sh)
#
# The release build + packaging step: the Rust core is cross-compiled for
# every ABI and the APK is signed, so it takes a few minutes. Delegate of
# ./build.sh --android — the same thing, one command shorter to type.
#
# Output: dist/mailclient-android/mailclient-native-<version>-release.apk
#         dist/mailclient-android/VERSION
#
# Put it on a phone or a running emulator with ./install-android.sh.
# For the fast dev loop (debug build on the emulator) use
# ./scripts/android-dev.sh --run instead.
set -euo pipefail
cd "$(dirname "$0")"

echo "==> native Android release build (this takes a few minutes)"
exec ./build.sh --android "$@"
