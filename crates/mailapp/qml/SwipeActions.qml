pragma Singleton

import QtQuick

// How a swipe action looks, in one place: the row's swipe cue and the
// Settings choices both name the actions this way. The values themselves
// (`none`, `archive`, ...) and their validation are mailcore's
// (`settings::normalize_swipe_action`); this only labels and styles them.
QtObject {
    function label(action) {
        switch (action) {
        case "archive":
            return qsTr("Archive");
        case "delete":
            return qsTr("Delete");
        case "mark_read":
            return qsTr("Mark as read");
        case "mark_unread":
            return qsTr("Mark as unread");
        case "star":
            return qsTr("Star");
        case "unstar":
            return qsTr("Remove star");
        }
        return qsTr("None");
    }

    function icon(action) {
        switch (action) {
        case "archive":
            return Icons.archive;
        case "delete":
            return Icons.trash;
        case "mark_read":
            return Icons.markRead;
        case "mark_unread":
            return Icons.markUnread;
        case "star":
            return Icons.star;
        case "unstar":
            return Icons.starBorder;
        }
        return "";
    }

    function color(action) {
        if (action === "delete")
            return Theme.danger;
        if (action === "star" || action === "unstar")
            return Theme.star;
        return Theme.accent;
    }
}
