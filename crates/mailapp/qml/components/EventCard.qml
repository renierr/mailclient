import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Event preview card displayed above the message body for calendar invitations.
Rectangle {
    id: root

    required property var event
    signal openClicked
    signal saveClicked

    radius: Theme.radius
    color: Theme.bgAlt
    border.width: 1
    border.color: (root.event && root.event.is_cancelled) ? Theme.danger : Theme.border
    implicitHeight: layout.implicitHeight + Theme.sm * 2

    ColumnLayout {
        id: layout

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: Theme.sm
        spacing: Theme.sm

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.md

            // Calendar icon badge
            Rectangle {
                Layout.alignment: Qt.AlignTop
                implicitWidth: Math.round(38 * Theme.uiScale)
                implicitHeight: Math.round(38 * Theme.uiScale)
                radius: Theme.radius
                color: (root.event && root.event.is_cancelled) ? (Theme.dark ? "#3d1414" : "#fee2e2") : Theme.accentBg

                Label {
                    anchors.centerIn: parent
                    text: Icons.event
                    font.family: Icons.fontFamily
                    font.pixelSize: Math.round(22 * Theme.uiScale)
                    color: (root.event && root.event.is_cancelled) ? Theme.danger : Theme.accent
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                spacing: Theme.xs

                // Summary + Cancelled badge
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.sm

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        text: root.event ? root.event.summary : ""
                        color: Theme.text
                        font.pixelSize: Theme.fontBase
                        font.bold: true
                        wrapMode: Text.Wrap
                    }

                    Rectangle {
                        visible: root.event && root.event.is_cancelled
                        radius: Theme.radiusSmall
                        color: Theme.danger
                        implicitWidth: cancelLabel.implicitWidth + Theme.xs * 2
                        implicitHeight: cancelLabel.implicitHeight + 2

                        Label {
                            id: cancelLabel

                            anchors.centerIn: parent
                            text: qsTr("Cancelled")
                            color: "#ffffff"
                            font.pixelSize: Theme.fontSmall
                            font.bold: true
                        }
                    }
                }

                // Time
                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.xs

                    Label {
                        text: Icons.schedule
                        font.family: Icons.fontFamily
                        font.pixelSize: Theme.fontSmall
                        color: Theme.textMuted
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        text: root.event ? root.event.formatted_time : ""
                        color: Theme.text
                        font.pixelSize: Theme.fontSmall
                        font.bold: true
                        wrapMode: Text.Wrap
                    }
                }

                // Location (if present)
                RowLayout {
                    Layout.fillWidth: true
                    visible: !!root.event && !!root.event.location
                    spacing: Theme.xs

                    Label {
                        text: Icons.place
                        font.family: Icons.fontFamily
                        font.pixelSize: Theme.fontSmall
                        color: Theme.textMuted
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        text: (root.event && root.event.location) ? root.event.location : ""
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                        elide: Text.ElideRight
                    }
                }

                // Organizer (if present)
                RowLayout {
                    Layout.fillWidth: true
                    visible: !!root.event && !!root.event.organizer
                    spacing: Theme.xs

                    Label {
                        text: Icons.person
                        font.family: Icons.fontFamily
                        font.pixelSize: Theme.fontSmall
                        color: Theme.textMuted
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        text: (root.event && root.event.organizer) ? qsTr("Organizer: %1").arg(root.event.organizer) :
                                                                     ""
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                        elide: Text.ElideRight
                    }
                }
            }
        }

        // Action buttons
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.sm

            Item {
                Layout.fillWidth: true
            }

            AppButton {
                text: qsTr("Open in Calendar")
                visible: !!root.event && root.event.attachment_id !== undefined && root.event.attachment_id !== null
                onClicked: root.openClicked()
            }

            AppButton {
                text: qsTr("Save .ics…")
                visible: !!root.event && root.event.attachment_id !== undefined && root.event.attachment_id !== null
                onClicked: root.saveClicked()
            }
        }
    }
}
