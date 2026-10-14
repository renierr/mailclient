pragma Singleton

import QtQuick

// A scheduled send's time in the user's words, in one place: the composer's
// status line, the outbox rows and the Send-later dialog all show it. The
// instant comes from the core as UTC RFC3339 (`send_at`); this only formats
// it in local time with the locale's short time. Whether a time is valid is
// the core's call (`send_at_error`), never decided here.
QtObject {
    function label(sendAtUtc) {
        if (!sendAtUtc)
            return "";
        var d = new Date(sendAtUtc);
        if (isNaN(d.getTime()))
            return "";
        return qsTr("Sends %1 at %2").arg(Qt.formatDate(d, "dddd, d MMMM")).arg(Qt.formatTime(d, Qt.locale().timeFormat(
                                                                                                     Locale.ShortFormat)));
    }
}
