import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// A mail attached to the open one (`.eml`, `message/rfc822`; mailcore::
// attached via the feed's `attached_messages`): subject, sender, date and a
// snippet, with its text body to expand, open or save. A card whose file is
// not cached yet (`loaded` false) offers the download that fills it in.
Rectangle {
    id: root

    required property var mail
    property bool downloading: false
    property bool expanded: false
    signal openClicked
    signal saveClicked
    signal downloadClicked

    readonly property bool loaded: !!root.mail && root.mail.loaded === true
    readonly property bool hasFile: !!root.mail && root.mail.attachment_id !== undefined
                                    && root.mail.attachment_id !== null

    radius: Theme.radius
    color: Theme.bgAlt
    border.width: 1
    border.color: Theme.border
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

            Rectangle {
                Layout.alignment: Qt.AlignTop
                implicitWidth: Math.round(38 * Theme.uiScale)
                implicitHeight: Math.round(38 * Theme.uiScale)
                radius: Theme.radius
                color: Theme.accentBg

                Label {
                    anchors.centerIn: parent
                    text: Icons.mail
                    font.family: Icons.fontFamily
                    font.pixelSize: Math.round(22 * Theme.uiScale)
                    color: Theme.accent
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                spacing: Theme.xs

                TextEdit {
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    text: root.mail ? root.mail.subject : ""
                    color: Theme.text
                    font.pixelSize: Theme.fontBase
                    font.bold: true
                    wrapMode: Text.Wrap
                    readOnly: true
                    selectByMouse: true
                }

                TextEdit {
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    visible: text !== ""
                    text: (root.mail && root.mail.byline) ? root.mail.byline : ""
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                    readOnly: true
                    selectByMouse: true
                }

                Label {
                    Layout.fillWidth: true
                    visible: !!root.mail && !!root.mail.to
                    text: (root.mail && root.mail.to) ? qsTr("To: %1").arg(root.mail.to) : ""
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    elide: Text.ElideRight
                }

                Label {
                    Layout.fillWidth: true
                    visible: !root.loaded
                    text: qsTr("Attached message not downloaded yet")
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                }

                Label {
                    Layout.fillWidth: true
                    visible: root.loaded && !root.expanded && root.mail.snippet !== ""
                    text: root.loaded ? root.mail.snippet : ""
                    color: Theme.text
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                }

                Label {
                    Layout.fillWidth: true
                    visible: root.loaded && root.mail.attachment_count > 0
                    text: root.loaded ? qsTr("%n attachment(s) inside", "", root.mail.attachment_count) : ""
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                }
            }
        }

        // The whole text body, selectable; plain text only (no remote
        // content, no layout) so expanding is always safe.
        TextEdit {
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            visible: root.expanded && root.loaded
            text: root.loaded && root.expanded ? root.mail.body_text : ""
            color: Theme.text
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.Wrap
            readOnly: true
            selectByMouse: true
        }

        Flow {
            Layout.fillWidth: true
            visible: root.hasFile
            spacing: Theme.sm
            layoutDirection: Qt.RightToLeft

            AppButton {
                text: qsTr("Save .eml…")
                onClicked: root.saveClicked()
            }

            AppButton {
                text: qsTr("Open")
                onClicked: root.openClicked()
            }

            AppButton {
                visible: root.loaded && root.mail.body_text !== ""
                text: root.expanded ? qsTr("Hide message") : qsTr("Show message")
                onClicked: root.expanded = !root.expanded
            }

            AppButton {
                visible: !root.loaded
                enabled: !root.downloading
                text: root.downloading ? qsTr("Downloading…") : qsTr("Download")
                onClicked: root.downloadClicked()
            }
        }
    }
}
