pragma Singleton

import QtQuick

// Pure helpers for the per-account form in Settings → Accounts & sync: how a
// stored override maps to a choice index and back, and which keys a draft
// changed. An empty value means "use the app-wide default", as in
// `mailcore::store::account_settings`.
//
// No UI here, so it stays headless-testable (`tst_AccountOverrides.qml`).
QtObject {
    // Interval choices after the leading "Default" entry, in minutes.
    readonly property var intervalSteps: [0, 5, 10, 15, 30, 60]

    // Choice index of an interval override: 0 = default, then the steps.
    function intervalIndex(value) {
        if (value === undefined || value === null || value === "")
            return 0;
        var idx = intervalSteps.indexOf(parseInt(value, 10));
        return idx >= 0 ? idx + 1 : 0;
    }

    function intervalValue(index) {
        return index <= 0 || index > intervalSteps.length ? "" : String(intervalSteps[index - 1]);
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

    // A quiet-hours time as stored ("HH:MM"), from "7:05" or "07:05"; ""
    // for anything else. Mirrors `account_settings::parse_time`.
    function timeValue(text) {
        var m = /^\s*(\d{1,2}):(\d{2})\s*$/.exec(text || "");
        if (!m)
            return "";
        var h = parseInt(m[1], 10);
        var min = parseInt(m[2], 10);
        if (h > 23 || min > 59)
            return "";
        return (h < 10 ? "0" : "") + h + ":" + m[2];
    }

    // The time to show for a stored value, or `fallback` when unset.
    function timeText(value, fallback) {
        var t = timeValue(value);
        return t !== "" ? t : fallback;
    }

    function isEmpty(obj) {
        for (var k in obj)
            return false;
        return true;
    }
}
