import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Contact preview card for a `.vcf` attachment (mailcore::vcard via the
// feed's `contacts`): name with its badge, title/organisation, addresses,
// numbers and the postal address, selectable for copying. A card whose file
// is not cached yet (`loaded` false) offers the download that fills it in.
Rectangle {
    id: root

    required property var contact
    property bool downloading: false
    signal openClicked
    signal saveClicked
    signal downloadClicked

    readonly property bool loaded: !!root.contact && root.contact.loaded === true
    readonly property bool hasFile: !!root.contact && root.contact.attachment_id !== undefined
                                    && root.contact.attachment_id !== null
    // One list of icon/value/label rows; the core already dropped blanks.
    readonly property var lines: {
        var out = [];
        if (!root.loaded)
            return out;
        var emails = root.contact.emails || [];
        for (var i = 0; i < emails.length; i++)
            out.push({
                         "icon": Icons.mail,
                         "value": emails[i].value,
                         "label": emails[i].label || ""
                     });
        var phones = root.contact.phones || [];
        for (var j = 0; j < phones.length; j++)
            out.push({
                         "icon": Icons.phone,
                         "value": phones[j].value,
                         "label": phones[j].label || ""
                     });
        if (root.contact.address)
            out.push({
                         "icon": Icons.place,
                         "value": root.contact.address,
                         "label": ""
                     });
        if (root.contact.url)
            out.push({
                         "icon": Icons.link,
                         "value": root.contact.url,
                         "label": ""
                     });
        return out;
    }

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

            Avatar {
                Layout.alignment: Qt.AlignTop
                visible: root.loaded
                implicitWidth: Math.round(38 * Theme.uiScale)
                implicitHeight: Math.round(38 * Theme.uiScale)
                badge: root.contact
            }

            Rectangle {
                Layout.alignment: Qt.AlignTop
                visible: !root.loaded
                implicitWidth: Math.round(38 * Theme.uiScale)
                implicitHeight: Math.round(38 * Theme.uiScale)
                radius: Theme.radius
                color: Theme.accentBg

                Label {
                    anchors.centerIn: parent
                    text: Icons.contacts
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
                    text: root.contact ? root.contact.name : ""
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
                    visible: !!root.contact && !!root.contact.affiliation
                    text: (root.contact && root.contact.affiliation) ? root.contact.affiliation : ""
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                    readOnly: true
                    selectByMouse: true
                }

                Label {
                    Layout.fillWidth: true
                    visible: !root.loaded
                    text: qsTr("Contact card not downloaded yet")
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                }

                Repeater {
                    model: root.lines

                    delegate: RowLayout {
                        id: line

                        required property var modelData

                        Layout.fillWidth: true
                        spacing: Theme.xs

                        Label {
                            Layout.alignment: Qt.AlignTop
                            text: line.modelData.icon
                            font.family: Icons.fontFamily
                            font.pixelSize: Theme.fontSmall
                            color: Theme.textMuted
                        }

                        TextEdit {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            text: line.modelData.value
                            color: Theme.text
                            font.pixelSize: Theme.fontSmall
                            wrapMode: Text.WrapAnywhere
                            readOnly: true
                            selectByMouse: true
                        }

                        Label {
                            Layout.alignment: Qt.AlignTop
                            visible: line.modelData.label !== ""
                            text: line.modelData.label
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontSmall
                        }
                    }
                }

                Label {
                    Layout.fillWidth: true
                    visible: !!root.contact && root.contact.more_cards > 0
                    text: root.contact ? qsTr("+%n more contact(s) in this file", "", root.contact.more_cards) : ""
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                }
            }
        }

        // Action buttons; Flow so a narrow pane wraps them.
        Flow {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignRight
            visible: root.hasFile
            spacing: Theme.sm
            layoutDirection: Qt.RightToLeft

            AppButton {
                text: qsTr("Save .vcf…")
                onClicked: root.saveClicked()
            }

            AppButton {
                text: qsTr("Open in Contacts")
                onClicked: root.openClicked()
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
