import QtQuick
import QtQuick.Controls

import Mailclient

// Themed popup menu. Items are plain `MenuItem`s; the styling comes from
// `delegate` here so callers stay declarative.
Menu {
    id: control

    implicitWidth: 220
    padding: Theme.xs

    background: Rectangle {
        radius: Theme.radius
        color: Theme.bgRaised
        border.width: 1
        border.color: Theme.border
    }

    delegate: MenuItem {
        id: item
        implicitHeight: 30

        contentItem: Text {
            text: item.text
            color: item.enabled ? Theme.text : Theme.textMuted
            font.pixelSize: Theme.fontBase
            verticalAlignment: Text.AlignVCenter
            leftPadding: Theme.sm
            rightPadding: Theme.sm
            elide: Text.ElideRight
        }

        background: Rectangle {
            radius: Theme.radius
            color: item.highlighted ? Theme.hover : "transparent"
        }
    }
}
