import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Unsent mail for the current account: queued, sending or failed sends.
// A sync retries everything still deliverable; a failed send the user
// already saw keeps no bytes and must be resent by hand — dismissing it
// here only forgets the dead row. Uses AppDialog like the other managers.
AppDialog {
    id: root
    title: qsTr("Outbox")
    preferredWidth: 600
    preferredHeight: 520
    minWidth: 420
    minHeight: 340
    padding: Theme.lg

    property var backend
    property var rows: []
    property var outboxStatus: ({})
    signal statusMessage(string text)
    signal syncRequested

    function reload() {
        root.rows = FeedJson.parse(root.backend.outbox_json(), []);
        root.outboxStatus = FeedJson.parse(root.backend.outbox_status_json(), ({}));
    }

    function dismissRow(id) {
        var r = root.backend.dismiss_outbox(id);
        if (r !== "")
            root.statusMessage(r);
        root.reload();
    }

    // A row's one-line state comes phrased from the core (`state`), so
    // both frontends show the same words.
    onOpened: root.reload()

    Connections {
        target: root.backend
        function onJob_finished(kind) {
            if ((kind === "Send" || kind === "Sync") && root.visible)
                root.reload();
        }
    }

    footer: RowLayout {
        spacing: Theme.sm
        AppButton {
            Layout.leftMargin: Theme.lg + 8
            Layout.bottomMargin: Theme.md
            text: qsTr("Sync now")
            Accessible.name: qsTr("Sync now to retry unsent mail")
            enabled: (root.outboxStatus.retryable || 0) > 0
            onClicked: root.syncRequested()
        }
        Item {
            Layout.fillWidth: true
        }
        AppButton {
            Layout.rightMargin: Theme.lg + 8
            Layout.bottomMargin: Theme.md
            text: qsTr("Close")
            Accessible.name: qsTr("Close outbox")
            onClicked: root.close()
        }
    }

    contentItem: ColumnLayout {
        spacing: Theme.md

        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            text: qsTr("Mail waiting to send, or sends that failed. Syncing retries everything still deliverable.")
        }

        Label {
            visible: root.rows.length === 0
            Layout.fillWidth: true
            Layout.topMargin: Theme.lg
            horizontalAlignment: Text.AlignHCenter
            color: Theme.textMuted
            text: qsTr("Outbox is empty")
        }

        ListView {
            id: outboxList
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: root.rows.length > 0
            model: root.rows
            clip: true
            spacing: Theme.xs
            activeFocusOnTab: true
            keyNavigationEnabled: true
            highlightFollowsCurrentItem: true

            delegate: Rectangle {
                id: rowDelegate
                width: ListView.view.width
                implicitHeight: Math.max(Math.round(64 * Theme.uiScale), outboxLayout.implicitHeight + Theme.sm * 2)
                color: outboxHover.hovered || ListView.isCurrentItem ? Theme.bgAlt : "transparent"
                radius: Theme.radius

                HoverHandler {
                    id: outboxHover
                }

                RowLayout {
                    id: outboxLayout
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: Theme.md
                    anchors.rightMargin: Theme.sm
                    spacing: Theme.sm

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 2

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            font.bold: true
                            font.pixelSize: Theme.fontMedium
                            color: Theme.text
                            text: modelData.subject
                        }

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            elide: Text.ElideRight
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontSmall
                            text: (modelData.envelope_to || []).join(", ")
                            visible: (modelData.envelope_to || []).length > 0
                        }

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontSmall
                            text: modelData.state
                        }

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            color: Theme.danger
                            font.pixelSize: Theme.fontSmall
                            text: modelData.last_error
                            visible: modelData.last_error !== ""
                        }
                    }

                    IconButton {
                        Layout.alignment: Qt.AlignVCenter
                        text: Icons.close
                        iconFont: true
                        tooltip: qsTr("Forget this entry")
                        Accessible.name: qsTr("Forget unsent mail to %1").arg((modelData.envelope_to || []).join(", "))
                        enabled: modelData.dismissable === true
                        onClicked: root.dismissRow(modelData.id)
                    }
                }
            }
        }
    }
}
