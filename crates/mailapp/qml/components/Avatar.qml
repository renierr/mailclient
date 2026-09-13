import QtQuick

import Mailclient

// Circle with the sender's initial, coloured deterministically from the name.
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
        font.pixelSize: Math.round(root.width * 0.42)
    }
}
