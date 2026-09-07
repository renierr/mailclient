import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Reader pane. `message` is a ListElement-like object with
// {subject, from, date, body}. M3 swaps the RichText Text for a
// sandboxed QtWebEngine view (remote content blocked).
Pane {
    id: root

    property var message
    signal replyRequested()
    signal forwardRequested()
    signal starRequested()
    signal deleteRequested()
    signal statusMessage(string text)

    padding: 8

    ColumnLayout {
        anchors.fill: parent
        spacing: 8
        visible: root.message !== undefined

        RowLayout {
            Layout.fillWidth: true
            Label {
                Layout.fillWidth: true
                text: root.message ? root.message.subject : ""
                font.pixelSize: 18
                font.bold: true
                wrapMode: Text.Wrap
            }
            ToolButton {
                text: qsTr("↩ Reply")
                onClicked: root.replyRequested()
            }
            ToolButton {
                text: qsTr("→ Forward")
                onClicked: root.forwardRequested()
            }
            ToolButton {
                text: qsTr("🗑")
                Accessible.name: qsTr("Delete")
                onClicked: root.deleteRequested()
            }
            ToolButton {
                text: qsTr("★")
                Accessible.name: qsTr("Star")
                onClicked: root.starRequested()
            }
        }

        Label {
            text: root.message ? qsTr("From: %1   •   %2").arg(root.message.from).arg(root.message.date) : ""
            opacity: 0.7
            font.pixelSize: 12
        }
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            opacity: 0.2
            color: palette.text
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            Text {
                width: root.width - 32
                text: root.message ? root.message.body : ""
                textFormat: Text.RichText
                wrapMode: Text.Wrap
                // Base color for HTML without explicit colors (theme-aware).
                color: palette.text
                linkColor: palette.link
                onLinkActivated: link => root.statusMessage(qsTr("Blocked remote link (M3 sandbox): %1").arg(link))
            }
        }
    }

    Label {
        anchors.centerIn: parent
        visible: root.message === undefined
        text: qsTr("Select a message")
        opacity: 0.5
    }
}
