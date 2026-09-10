import QtQuick
import QtQuick.Controls

import Mailclient

// Flat square button used for every toolbar / reader action. Exists so hover,
// pressed and disabled states are consistent everywhere instead of each pane
// re-inventing a ToolButton look.
AbstractButton {
    id: root

    property string tooltip: ""
    property color contentColor: Theme.text
    property int fontSize: Theme.fontMedium
    property bool active: false

    implicitWidth: 32
    implicitHeight: 32
    hoverEnabled: true
    opacity: enabled ? 1 : 0.35

    background: Rectangle {
        radius: Theme.radius
        color: root.active ? Theme.selected
             : root.pressed ? Theme.border
             : root.hovered ? Theme.hover
             : "transparent"
    }

    contentItem: Text {
        text: root.text
        color: root.active ? Theme.accent : root.contentColor
        font.pixelSize: root.fontSize
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    ToolTip.visible: root.hovered && root.tooltip !== ""
    ToolTip.text: root.tooltip
    ToolTip.delay: 500
}
