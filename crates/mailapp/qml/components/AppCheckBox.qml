import QtQuick
import QtQuick.Controls

import Mailclient

// Themed checkbox with a wrapping label, for the settings rows.
CheckBox {
    id: control

    property string hint: ""

    font.pixelSize: Theme.fontBase
    spacing: Theme.sm

    indicator: Rectangle {
        implicitWidth: 18
        implicitHeight: 18
        x: control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        radius: Theme.xs
        color: control.checked ? Theme.accent : Theme.bg
        border.width: 1
        border.color: control.checked ? Theme.accent
                    : control.hovered ? Theme.accent
                    : Theme.border

        Text {
            anchors.centerIn: parent
            visible: control.checked
            text: "✓"
            color: Theme.accentText
            font.pixelSize: Theme.fontSmall
            font.bold: true
        }
    }

    contentItem: Text {
        text: control.text
        color: Theme.text
        font: control.font
        wrapMode: Text.Wrap
        verticalAlignment: Text.AlignVCenter
        leftPadding: control.indicator.width + control.spacing
    }
}
