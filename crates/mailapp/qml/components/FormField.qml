import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Labelled text input for the dialogs: label, field, optional hint.
// Fixes flaw F4 (placeholder-only fields with no labels) in one place.
//
// Where the surrounding layout already provides the label — the composer's
// header grid — use AppTextField directly instead, or you get two labels.
ColumnLayout {
    id: root

    property alias label: labelItem.text
    property alias text: field.text
    property alias placeholderText: field.placeholderText
    // A secret: masked, with a reveal toggle inside the field.
    property bool password: false
    property alias validator: field.validator
    property alias inputMethodHints: field.inputMethodHints
    property alias field: field
    property string hint: ""
    property bool required: false
    property bool invalid: false
    property bool revealed: false

    signal accepted
    signal editingFinished

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
        Item {
            Layout.fillWidth: true
        }
    }

    AppTextField {
        id: field
        Layout.fillWidth: true
        invalid: root.invalid
        echoMode: root.password && !root.revealed ? TextInput.Password : TextInput.Normal
        rightPadding: root.password ? revealButton.width + Theme.xs : Theme.sm
        onAccepted: root.accepted()
        onEditingFinished: root.editingFinished()

        IconButton {
            id: revealButton
            visible: root.password
            anchors.right: parent.right
            anchors.rightMargin: 2
            anchors.verticalCenter: parent.verticalCenter
            implicitWidth: Theme.controlHeight - 4
            implicitHeight: Theme.controlHeight - 4
            iconFont: true
            text: root.revealed ? Icons.visibilityOff : Icons.visibility
            tooltip: root.revealed ? qsTr("Hide password") : qsTr("Show password")
            contentColor: Theme.textMuted
            onClicked: root.revealed = !root.revealed
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
