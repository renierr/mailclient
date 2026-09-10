import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Labelled text input for the dialogs. Fixes flaw F4 (placeholder-only fields
// with no labels) in one place, and gives every dialog the same field look.
ColumnLayout {
    id: root

    property alias label: labelItem.text
    property alias text: field.text
    property alias placeholderText: field.placeholderText
    property alias echoMode: field.echoMode
    property alias validator: field.validator
    property alias inputMethodHints: field.inputMethodHints
    property alias field: field
    property string hint: ""
    property bool required: false
    property bool invalid: false

    signal accepted()
    signal editingFinished()

    spacing: Theme.xs

    RowLayout {
        spacing: Theme.xs
        Label {
            id: labelItem
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
        }
        Label {
            text: "*"
            visible: root.required
            color: Theme.danger
            font.pixelSize: Theme.fontSmall
        }
        Item { Layout.fillWidth: true }
    }

    TextField {
        id: field
        Layout.fillWidth: true
        color: Theme.text
        placeholderTextColor: Theme.textMuted
        font.pixelSize: Theme.fontBase
        selectByMouse: true
        leftPadding: Theme.sm
        rightPadding: Theme.sm
        onAccepted: root.accepted()
        onEditingFinished: root.editingFinished()

        background: Rectangle {
            radius: Theme.radius
            color: Theme.bg
            border.width: 1
            border.color: root.invalid ? Theme.danger
                        : field.activeFocus ? Theme.accent
                        : Theme.border
        }
    }

    Label {
        text: root.hint
        visible: root.hint !== ""
        color: Theme.textMuted
        font.pixelSize: Theme.fontTiny
        wrapMode: Text.Wrap
        Layout.fillWidth: true
    }
}
