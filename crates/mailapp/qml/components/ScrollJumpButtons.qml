import QtQuick

import Mailclient

// Jump to the top / bottom of a long list: a small pair floating over its
// bottom-right corner. Each button shows only while that end is out of view,
// and neither until the list is a couple of screens long.
Column {
    id: root

    required property ListView target

    readonly property bool longList: target.contentHeight > target.height * 2

    spacing: Theme.xs
    visible: longList

    component JumpButton: Rectangle {
        id: jump

        property alias glyph: button.text
        property alias tooltip: button.tooltip
        signal clicked

        width: Theme.controlHeight
        height: Theme.controlHeight
        radius: Theme.radius
        color: Theme.bgRaised
        border.width: 1
        border.color: Theme.border

        IconButton {
            id: button
            anchors.fill: parent
            iconFont: true
            contentColor: Theme.textMuted
            onClicked: jump.clicked()
        }
    }

    JumpButton {
        visible: !root.target.atYBeginning
        glyph: Icons.verticalAlignTop
        tooltip: qsTr("Jump to top")
        onClicked: root.target.positionViewAtBeginning()
    }
    JumpButton {
        visible: !root.target.atYEnd
        glyph: Icons.verticalAlignBottom
        tooltip: qsTr("Jump to bottom")
        onClicked: root.target.positionViewAtEnd()
    }
}
