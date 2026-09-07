import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Settings dialog. Values are display-only in M0; M4 persists them.
Dialog {
    id: root
    title: qsTr("Settings")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 440, 440)
    anchors.centerIn: parent
    standardButtons: Dialog.Ok | Dialog.Cancel

    signal statusMessage(string text)

    GridLayout {
        anchors.fill: parent
        columns: 2
        columnSpacing: 12
        rowSpacing: 8

        Label { text: qsTr("Check mail every") }
        ComboBox {
            Layout.fillWidth: true
            model: [qsTr("1 minute"), qsTr("5 minutes"), qsTr("15 minutes"), qsTr("Manually")]
            currentIndex: 1
        }
        Label { text: qsTr("Load remote images") }
        CheckBox { checked: false }
        Label { text: qsTr("Database") }
        Label {
            Layout.fillWidth: true
            text: qsTr("~/.local/share/mailclient/mailclient.sqlite")
            opacity: 0.7
            elide: Text.ElideLeft
            font.pixelSize: 12
        }
    }

    onAccepted: root.statusMessage(qsTr("Settings persist in M4"))
}
