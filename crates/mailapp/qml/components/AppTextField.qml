import QtQuick
import QtQuick.Controls

import Mailclient

// The themed single-line input. Used directly where a layout already
// supplies the label (the composer's header grid), and wrapped by
// FormField.qml where a label + hint belong with the field.
TextField {
    id: control

    property bool invalid: false

    implicitHeight: 32
    color: Theme.text
    placeholderTextColor: Theme.textMuted
    font.pixelSize: Theme.fontBase
    selectByMouse: true
    leftPadding: Theme.sm
    rightPadding: Theme.sm

    background: Rectangle {
        radius: Theme.radius
        color: Theme.bg
        border.width: 1
        border.color: control.invalid ? Theme.danger
                    : control.activeFocus ? Theme.accent
                    : Theme.border
    }
}
