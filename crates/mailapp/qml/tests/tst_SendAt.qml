import QtQuick
import QtTest

import Mailclient

TestCase {
    name: "SendAt"

    function test_empty_and_unreadable_give_no_label() {
        compare(SendAt.label(""), "");
        compare(SendAt.label(undefined), "");
        compare(SendAt.label("not a time"), "");
    }

    function test_a_utc_instant_reads_in_local_time() {
        var at = new Date(2030, 0, 15, 8, 30);
        var text = SendAt.label(at.toISOString());
        verify(text.indexOf(Qt.formatDate(at, "dddd, d MMMM")) >= 0, text);
        verify(text.indexOf(Qt.formatTime(at, Qt.locale().timeFormat(Locale.ShortFormat))) >= 0, text);
    }
}
