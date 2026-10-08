import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Delivery report card for a bounce (mailcore::report via the feed's
// `report`): outcome, what it means, each recipient with the reason in plain
// words and the server's own text, the original's subject, and "Edit &
// resend" when the sent original is cached. A report whose status part is
// not cached yet (`loaded` false) offers the download that fills it in.
Rectangle {
    id: root

    required property var report
    property bool downloading: false
    signal resendClicked
    signal downloadClicked

    readonly property bool loaded: !!root.report && root.report.loaded === true
    readonly property string outcome: root.report ? root.report.outcome : ""
    readonly property color tone: root.loaded ? root.toneFor(root.outcome) : Theme.accent

    // `outcome` / `action` from mailcore::report to a theme colour.
    function toneFor(action) {
        if (action === "failed")
            return Theme.danger;
        if (action === "delayed")
            return Theme.warning;
        return Theme.success;
    }

    radius: Theme.radius
    color: Theme.bgAlt
    border.width: 1
    border.color: root.loaded && root.outcome === "failed" ? Theme.danger : Theme.border
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

            Label {
                Layout.alignment: Qt.AlignTop
                text: !root.loaded || root.outcome === "failed" ? Icons.error : root.outcome === "delayed"
                                                                                ? Icons.schedule : Icons.checkCircle
                font.family: Icons.fontFamily
                font.pixelSize: Math.round(26 * Theme.uiScale)
                color: root.tone
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                spacing: Theme.xs

                Label {
                    Layout.fillWidth: true
                    text: root.report ? root.report.title : ""
                    color: root.tone
                    font.pixelSize: Theme.fontBase
                    font.bold: true
                    wrapMode: Text.Wrap
                }

                Label {
                    Layout.fillWidth: true
                    text: root.report ? root.report.detail : ""
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                }

                Label {
                    Layout.fillWidth: true
                    visible: !!root.report && !!root.report.original_subject
                    text: (root.report && root.report.original_subject)
                          ? qsTr("Original: %1").arg(root.report.original_subject) : ""
                    color: Theme.text
                    font.pixelSize: Theme.fontSmall
                    elide: Text.ElideRight
                }
            }
        }

        Repeater {
            model: root.loaded ? root.report.recipients : []

            delegate: ColumnLayout {
                id: recipient

                required property var modelData

                Layout.fillWidth: true
                Layout.leftMargin: Theme.sm
                spacing: 2

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.sm

                    TextEdit {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        text: recipient.modelData.address
                        color: Theme.text
                        font.pixelSize: Theme.fontSmall
                        font.bold: true
                        wrapMode: Text.WrapAnywhere
                        readOnly: true
                        selectByMouse: true
                    }

                    Label {
                        text: recipient.modelData.action_label
                        color: root.toneFor(recipient.modelData.action)
                        font.pixelSize: Theme.fontSmall
                    }
                }

                Label {
                    Layout.fillWidth: true
                    visible: !!recipient.modelData.reason
                    text: recipient.modelData.reason || ""
                    color: Theme.text
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                }

                TextEdit {
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    visible: !!recipient.modelData.diagnostic
                    text: recipient.modelData.diagnostic || ""
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    wrapMode: Text.Wrap
                    readOnly: true
                    selectByMouse: true
                }
            }
        }

        Flow {
            Layout.fillWidth: true
            visible: (!!root.report && root.report.can_resend) || !root.loaded
            spacing: Theme.sm
            layoutDirection: Qt.RightToLeft

            AppButton {
                visible: !!root.report && root.report.can_resend
                text: qsTr("Edit && resend…")
                onClicked: root.resendClicked()
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
