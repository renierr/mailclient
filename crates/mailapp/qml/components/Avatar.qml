import QtQuick

import Mailclient

// Circle with the sender's initials, coloured deterministically from the seed.
Rectangle {
    id: root

    property string initials: "?"
    property string seed: initials

    implicitWidth: Math.round(34 * Theme.uiScale)
    implicitHeight: Math.round(34 * Theme.uiScale)
    width: implicitWidth
    height: implicitHeight
    radius: width / 2
    color: Theme.avatarColor(root.seed)

    Text {
        anchors.centerIn: parent
        text: root.initials
        color: "white"
        font.bold: true
        // Two letters need a smaller size to stay inside the circle.
        font.pixelSize: Math.round(root.width * (root.initials.length > 1 ? 0.36 : 0.42))
    }
}
