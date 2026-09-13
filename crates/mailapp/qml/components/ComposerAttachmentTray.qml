import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Attachment list chip display for the composer dialog.
Rectangle {
    id: root

    property var attachments: []
    signal addRequested()
    signal removeRequested(int index)

    implicitHeight: attachRow.implicitHeight + Theme.sm * 2
    radius: Theme.radius
    color: Theme.bgAlt
    border.width: 1
    border.color: Theme.border

    RowLayout {
        id: attachRow
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Theme.sm
        spacing: Theme.xs

        Label {
            text: "📎"
        }
        Label {
            text: qsTr("%n file(s)", "", root.attachments ? root.attachments.length : 0)
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
        }
        // Chips wrap via Flow (names can be long).
        Flow {
            Layout.fillWidth: true
            spacing: Theme.xs
            Repeater {
                model: root.attachments
                Rectangle {
                    id: chipBox
                    height: Math.round(26 * Theme.uiScale)
                    width: chipRow.implicitWidth + Theme.sm * 2
                    radius: 13
                    color: Theme.bgRaised
                    border.width: 1
                    border.color: Theme.border
                    required property var modelData
                    required property int index
                    Row {
                        id: chipRow
                        anchors.centerIn: parent
                        spacing: 4
                        Label {
                            anchors.verticalCenter: parent.verticalCenter
                            text: chipBox.modelData.name
                            color: Theme.text
                            font.pixelSize: Theme.fontSmall
                            elide: Text.ElideMiddle
                            width: Math.min(implicitWidth, 180)
                        }
                        IconButton {
                            anchors.verticalCenter: parent.verticalCenter
                            width: Math.round(20 * Theme.uiScale)
                            height: Math.round(20 * Theme.uiScale)
                            fontSize: Theme.fontSmall
                            text: "✕"
                            tooltip: qsTr("Remove")
                            onClicked: root.removeRequested(chipBox.index)
                        }
                    }
                }
            }
        }
        AppButton {
            text: qsTr("Add")
            onClicked: root.addRequested()
        }
    }
}
