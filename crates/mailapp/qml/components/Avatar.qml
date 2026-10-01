import QtQuick

import Mailclient

// Circle with a sender's badge. `badge` is any feed row carrying the
// mailcore badge fields (`initials`, `avatar_light`, `avatar_dark`, see
// mailcore::badge) -- a list row's model, the reader payload, an account.
Rectangle {
    id: root

    property var badge: null
    readonly property string initials: (root.badge && root.badge.initials) || "?"

    implicitWidth: Math.round(34 * Theme.uiScale)
    implicitHeight: Math.round(34 * Theme.uiScale)
    width: implicitWidth
    height: implicitHeight
    radius: width / 2
    color: (root.badge && (Theme.dark ? root.badge.avatar_dark : root.badge.avatar_light)) || Theme.textMuted

    Text {
        anchors.centerIn: parent
        text: root.initials
        color: "white"
        font.bold: true
        // Two letters need a smaller size to stay inside the circle.
        font.pixelSize: Math.round(root.width * (root.initials.length > 1 ? 0.36 : 0.42))
    }
}
