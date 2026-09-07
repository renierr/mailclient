#!/usr/bin/env bash
# Headless smoke test: boots the app offscreen and FAILS on any QML load error.
# (QQmlApplicationEngine idles even with no root object, so mere "stays alive"
# proves nothing — the log grep is the actual assertion.)
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
check() {
    local name="$1"; shift
    local log="$1"; shift
    if grep -qiE "failed to load|not a type|QQmlApplicationEngine failed|error" "$log"; then
        echo "FAIL($name): QML errors:"
        grep -iE "failed to load|not a type|QQmlApplicationEngine failed|error" "$log" | head -n 10
        fail=1
    else
        echo "OK($name)"
    fi
}

echo "== dist bundle (filesystem QML) =="
MAILCLIENT_DB=/tmp/smoke-dist.sqlite QT_QPA_PLATFORM=offscreen \
    timeout 8 ./dist/mailclient/bin/mailapp > /tmp/smoke-dist.log 2>&1 || true
check "dist" /tmp/smoke-dist.log

echo "== isolated binary (embedded QML module) =="
rm -rf /tmp/smoke-iso && mkdir -p /tmp/smoke-iso
cp dist/mailclient/bin/mailapp /tmp/smoke-iso/
MAILCLIENT_DB=/tmp/smoke-iso.sqlite QT_QPA_PLATFORM=offscreen \
    timeout 8 /tmp/smoke-iso/mailapp > /tmp/smoke-iso.log 2>&1 || true
check "embedded" /tmp/smoke-iso.log

rm -rf /tmp/smoke-iso /tmp/smoke-iso.sqlite* /tmp/smoke-dist.sqlite* /tmp/smoke-dist.log /tmp/smoke-iso.log
exit $fail
