pragma Singleton

import QtQuick

// An outbox time in the user's words, in one place: the composer's status
// line, the outbox rows and the Send-later dialog all show them. Instants
// come from the core as UTC RFC3339 (`send_at`, `retry_at`); this only
// formats them in local time with the locale's short time. Whether a time
// is valid, and when a retry is due, is the core's call, never decided here.
QtObject {
    function parts(utc) {
        if (!utc)
            return null;
        var d = new Date(utc);
        if (isNaN(d.getTime()))
            return null;
        return [Qt.formatDate(d, "dddd, d MMMM"), Qt.formatTime(d, Qt.locale().timeFormat(Locale.ShortFormat))];
    }

    // "Sends Tuesday, 14 October at 08:00" for a scheduled send.
    function label(sendAtUtc) {
        var p = parts(sendAtUtc);
        return p ? qsTr("Sends %1 at %2").arg(p[0]).arg(p[1]) : "";
    }

    // "Next try Tuesday, 14 October at 08:15" for a send that failed.
    function nextTry(retryAtUtc) {
        var p = parts(retryAtUtc);
        return p ? qsTr("Next try %1 at %2").arg(p[0]).arg(p[1]) : "";
    }
}
