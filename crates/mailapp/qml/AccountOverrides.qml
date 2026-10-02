pragma Singleton

import QtQuick

// Pure helpers for the per-account form in Settings → Accounts & sync: how a
// stored override maps to a choice index and back, and which keys a draft
// changed. An empty value means "use the app-wide default", as in
// `mailcore::store::account_settings`.
//
// No UI here, so it stays headless-testable (`tst_AccountOverrides.qml`).
QtObject {
    // Choice index of an interval override: 0 = default, then `steps`, the
    // minutes the core offers (`settings::choices`).
    function intervalIndex(value, steps) {
        if (value === undefined || value === null || value === "")
            return 0;
        var idx = steps.indexOf(parseInt(value, 10));
        return idx >= 0 ? idx + 1 : 0;
    }

    function intervalValue(index, steps) {
        return index <= 0 || index > steps.length ? "" : String(steps[index - 1]);
    }

    // Yes/no override: 0 = default, 1 = on, 2 = off.
    function flagIndex(value) {
        return value === "1" ? 1 : (value === "0" ? 2 : 0);
    }

    function flagValue(index) {
        return ["", "1", "0"][index] || "";
    }

    // Keys whose draft value differs from the saved one, mapped to the
    // draft value ("" = inherit again).
    function changes(draft, saved) {
        var keys = {};
        var k;
        for (k in draft)
            keys[k] = true;
        for (k in saved)
            keys[k] = true;
        var out = {};
        for (k in keys) {
            var now = (draft && draft[k]) || "";
            var was = (saved && saved[k]) || "";
            if (now !== was)
                out[k] = now;
        }
        return out;
    }

    function isEmpty(obj) {
        for (var k in obj)
            return false;
        return true;
    }
}
