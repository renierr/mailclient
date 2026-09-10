import QtQuick
import QtQuick.Controls

import Mailclient

// Themed ComboBox: same radius, border and popup treatment as the text
// fields it sits next to.
ComboBox {
    id: control

    implicitHeight: 32
    font.pixelSize: Theme.fontBase

    background: Rectangle {
        radius: Theme.radius
        color: Theme.bg
        border.width: 1
        border.color: control.activeFocus || control.hovered ? Theme.accent : Theme.border
    }

    contentItem: Text {
        text: control.displayText
        font: control.font
        color: Theme.text
        verticalAlignment: Text.AlignVCenter
        leftPadding: Theme.sm
        rightPadding: control.indicator.width
        elide: Text.ElideRight
    }

    indicator: Text {
        x: control.width - width - Theme.sm
        y: control.topPadding + (control.availableHeight - height) / 2
        text: "⌄"
        color: Theme.textMuted
        font.pixelSize: Theme.fontBase
    }

    popup: Popup {
        y: control.height + 2
        width: control.width
        implicitHeight: Math.min(contentItem.implicitHeight + 2, 240)
        padding: 1

        background: Rectangle {
            radius: Theme.radius
            color: Theme.bgRaised
            border.width: 1
            border.color: Theme.border
        }

        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: control.delegateModel
            currentIndex: control.highlightedIndex
            ScrollBar.vertical: ScrollBar {}
        }
    }

    delegate: ItemDelegate {
        id: item
        width: control.width
        implicitHeight: 30
        required property var model
        required property int index

        contentItem: Text {
            text: item.model[control.textRole] !== undefined ? item.model[control.textRole]
                                                             : item.model.modelData
            color: Theme.text
            font.pixelSize: Theme.fontBase
            verticalAlignment: Text.AlignVCenter
            leftPadding: Theme.sm
            elide: Text.ElideRight
        }

        background: Rectangle {
            color: control.highlightedIndex === item.index ? Theme.selected
                 : item.hovered ? Theme.hover
                 : "transparent"
        }
    }
}
