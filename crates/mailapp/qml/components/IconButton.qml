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
    // True when `text` is an `Icons.*` glyph rather than a text symbol.
    property bool iconFont: false

    implicitWidth: Theme.controlHeight
    implicitHeight: Theme.controlHeight
    hoverEnabled: true
    // Tab-reachable, but a click must not steal focus (the composer toolbar
    // formats a selection that lives in the editor).
    focusPolicy: Qt.TabFocus
    opacity: enabled ? 1 : 0.35

    // `text` is usually an icon-font glyph, which a screen reader would read
    // out as a private-use character: the tooltip is the real name.
    Accessible.name: root.tooltip !== "" ? root.tooltip : root.text
    Accessible.role: Accessible.Button
    // Space activates any AbstractButton; Enter should too.
    Keys.onReturnPressed: root.click()
    Keys.onEnterPressed: root.click()

    background: Rectangle {
        radius: Theme.radius
        color: root.active ? Theme.selected
             : root.pressed ? Theme.border
             : root.hovered ? Theme.hover
             : "transparent"
        // Keyboard focus must be visible, or Tab lands somewhere unseen.
        border.width: root.visualFocus ? 2 : 0
        border.color: Theme.accent
    }

    contentItem: Text {
        text: root.text
        color: root.active ? Theme.accent : root.contentColor
        font.family: root.iconFont ? Icons.fontFamily : ""
        font.pixelSize: root.fontSize
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }

    ToolTip.visible: root.hovered && root.tooltip !== ""
    ToolTip.text: root.tooltip
    ToolTip.delay: 500
}
