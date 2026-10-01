import QtQuick
import QtTest

import Mailclient

// Headless unit tests for the AccountOverrides singleton (per-account
// settings form mapping). Run via `scripts/qml-check.sh`.
TestCase {
    name: "AccountOverrides"

    function test_interval_round_trips_through_the_choice_index() {
        compare(AccountOverrides.intervalIndex(""), 0);
        compare(AccountOverrides.intervalIndex(undefined), 0);
        compare(AccountOverrides.intervalIndex("0"), 1);
        compare(AccountOverrides.intervalIndex("30"), 5);
        compare(AccountOverrides.intervalIndex("7"), 0, "an unknown step shows the default");
        compare(AccountOverrides.intervalValue(0), "");
        compare(AccountOverrides.intervalValue(1), "0");
        compare(AccountOverrides.intervalValue(6), "60");
        compare(AccountOverrides.intervalValue(9), "");
    }

    function test_quiet_times_normalize_like_the_core() {
        compare(AccountOverrides.timeValue("7:05"), "07:05");
        compare(AccountOverrides.timeValue(" 23:59 "), "23:59");
        var bad = ["", "7", "24:00", "07:60", "07:5", "a:00", "007:00"];
        for (var i = 0; i < bad.length; i++)
            compare(AccountOverrides.timeValue(bad[i]), "", bad[i]);
        compare(AccountOverrides.timeValue(undefined), "");
        compare(AccountOverrides.timeText("", "07:00"), "07:00");
        compare(AccountOverrides.timeText("6:30", "07:00"), "06:30");
    }

    function test_flags_round_trip_through_the_choice_index() {
        compare(AccountOverrides.flagIndex(""), 0);
        compare(AccountOverrides.flagIndex("1"), 1);
        compare(AccountOverrides.flagIndex("0"), 2);
        compare(AccountOverrides.flagValue(0), "");
        compare(AccountOverrides.flagValue(1), "1");
        compare(AccountOverrides.flagValue(2), "0");
    }

    function test_changes_lists_only_what_the_draft_moved() {
        var saved = {
            "sync_interval_minutes": "30",
            "sent_copy_enabled": "0"
        };
        var draft = {
            "sync_interval_minutes": "30",
            "sent_copy_enabled": "",
            "collect_sent_contacts": "1"
        };
        var out = AccountOverrides.changes(draft, saved);
        compare(Object.keys(out).sort(), ["collect_sent_contacts", "sent_copy_enabled"]);
        compare(out["sent_copy_enabled"], "", "back to the default");
        compare(out["collect_sent_contacts"], "1");
        verify(AccountOverrides.isEmpty(AccountOverrides.changes(saved, saved)));
        verify(!AccountOverrides.isEmpty(out));
    }
}
