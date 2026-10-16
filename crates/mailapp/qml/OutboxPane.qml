import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// The account's outbox as a pane in place of the message list, opened from
// the sidebar's Outbox entry or the status-bar pill: scheduled sends, mail
// waiting for the next sync, retries and sends that gave up. What each row
// is, its words and which actions it offers all come from the core
// (`outbox::list_json`: `kind`, `state`, `actions`); this file only lays
// them out. Edit takes a mail back into the composer (`outbox_edit`).
Rectangle {
    id: root
    color: Theme.bg

    property var backend
    property int accountId: -1
    property bool showBack: false
    property var rows: []
    property var outboxStatus: ({})
    signal statusMessage(string text)
    signal syncRequested
    signal scheduleChanged
    signal backRequested
    // Main checks the composer is free, then takes the row out
    // (`outbox_edit`) and opens it there.
    signal editRequested(double id)

    // Row waiting for its reschedule pick.
    property double rescheduleId: -1

    function reload() {
        if (!root.backend || root.accountId < 0) {
            root.rows = [];
            root.outboxStatus = ({});
            return;
        }
        root.rows = FeedJson.parse(root.backend.outbox_json(root.accountId), []);
        root.outboxStatus = FeedJson.parse(root.backend.outbox_status_json(root.accountId), ({}));
    }

    function changed(message) {
        if (message !== "")
            root.statusMessage(message);
        root.reload();
        root.scheduleChanged();
    }

    function sendNowRow(id) {
        var r = root.backend.send_now_outbox(root.accountId, id);
        root.changed(r !== "" ? r : qsTr("Sending now"));
    }

    function deleteRow(id) {
        root.changed(root.backend.dismiss_outbox(root.accountId, id));
    }

    onVisibleChanged: {
        if (visible)
            root.reload();
    }
    onAccountIdChanged: root.reload()

    Connections {
        target: root.backend
        function onJob_finished(kind) {
            if ((kind === "Send" || kind === "Scheduled send" || kind === "Sync") && root.visible)
                root.reload();
        }
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Theme.md
        spacing: Theme.sm

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.sm

            IconButton {
                visible: root.showBack
                text: Icons.arrowBack
                iconFont: true
                tooltip: qsTr("Back to folders")
                onClicked: root.backRequested()
            }
            Label {
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                elide: Text.ElideRight
                text: qsTr("Outbox")
                color: Theme.text
                font.pixelSize: Theme.fontMedium
                font.bold: true
            }
            AppButton {
                text: qsTr("Sync now")
                Accessible.name: qsTr("Sync now to send waiting mail")
                enabled: (root.outboxStatus.retryable || 0) > 0
                onClicked: root.syncRequested()
            }
        }

        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            text: qsTr(
                      "Mail waiting to be sent from this device. Failed sends try again by themselves for a while, then stop and wait for you.")
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
            ScrollBar.vertical: ScrollBar {
                policy: ScrollBar.AsNeeded
            }

            delegate: Rectangle {
                id: rowDelegate
                width: ListView.view.width
                implicitHeight: rowLayout.implicitHeight + Theme.sm * 2
                color: rowHover.hovered ? Theme.bgAlt : "transparent"
                radius: Theme.radius
                border.width: modelData.kind === "failed" ? 1 : 0
                border.color: Theme.danger

                required property var modelData
                readonly property var actions: modelData.actions || ({})

                HoverHandler {
                    id: rowHover
                }

                ColumnLayout {
                    id: rowLayout
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
                    anchors.margins: Theme.sm
                    anchors.leftMargin: Theme.md
                    spacing: 2

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.sm
                        Label {
                            text: {
                                switch (rowDelegate.modelData.kind) {
                                case "scheduled":
                                    return Icons.schedule;
                                case "failed":
                                    return Icons.error;
                                default:
                                    return Icons.outbox;
                                }
                            }
                            color: rowDelegate.modelData.kind === "failed" ? Theme.danger : Theme.textMuted
                            font.family: Icons.fontFamily
                            font.pixelSize: Theme.fontBase
                        }
                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            font.bold: true
                            font.pixelSize: Theme.fontMedium
                            color: Theme.text
                            text: rowDelegate.modelData.subject
                        }
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                        text: (rowDelegate.modelData.envelope_to || []).join(", ")
                        visible: (rowDelegate.modelData.envelope_to || []).length > 0
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                        color: Theme.accent
                        font.pixelSize: Theme.fontSmall
                        text: SendAt.label(rowDelegate.modelData.send_at)
                        visible: (rowDelegate.modelData.send_at || "") !== ""
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                        text: SendAt.nextTry(rowDelegate.modelData.retry_at)
                        visible: (rowDelegate.modelData.retry_at || "") !== ""
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                        color: Theme.accent
                        font.pixelSize: Theme.fontSmall
                        text: qsTr("Mails a delivery receipt to me when it sends")
                        visible: rowDelegate.modelData.notify_self === true
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                        color: rowDelegate.modelData.kind === "failed" ? Theme.danger : Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                        text: rowDelegate.modelData.state
                    }

                    Label {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                        color: Theme.danger
                        font.pixelSize: Theme.fontSmall
                        text: rowDelegate.modelData.last_error
                        visible: rowDelegate.modelData.last_error !== ""
                    }

                    // Wraps under itself when the pane is narrow.
                    Flow {
                        Layout.fillWidth: true
                        Layout.topMargin: Theme.xs
                        spacing: Theme.xs
                        visible: sendNowButton.visible || rescheduleButton.visible || editButton.visible
                                 || deleteButton.visible

                        AppButton {
                            id: sendNowButton
                            visible: rowDelegate.actions.can_send_now === true
                            text: rowDelegate.actions.send_now_label || qsTr("Send now")
                            onClicked: root.sendNowRow(rowDelegate.modelData.id)
                        }
                        AppButton {
                            id: rescheduleButton
                            visible: rowDelegate.actions.can_reschedule === true
                            text: qsTr("Reschedule")
                            onClicked: {
                                root.rescheduleId = rowDelegate.modelData.id;
                                rescheduleDialog.openFor(rowDelegate.modelData.send_at || "");
                            }
                        }
                        AppButton {
                            id: editButton
                            visible: rowDelegate.actions.can_edit === true
                            text: qsTr("Edit")
                            Accessible.name: qsTr("Edit: take this mail back into the composer")
                            onClicked: root.editRequested(rowDelegate.modelData.id)
                        }
                        AppButton {
                            id: deleteButton
                            visible: rowDelegate.actions.can_delete === true
                            text: qsTr("Delete")
                            intent: "danger"
                            onClicked: {
                                deleteConfirm.rowId = rowDelegate.modelData.id;
                                deleteConfirm.subject = rowDelegate.modelData.subject;
                                deleteConfirm.open();
                            }
                        }
                    }
                }
            }
        }
    }

    ScheduleDialog {
        id: rescheduleDialog
        backend: root.backend
        // Rescheduling keeps whatever the row already asks for (its own
        // `notify_self` survives the move), so the toggle is not offered.
        showReceipt: false
        onScheduleAccepted: sendAt => {
            var r = root.backend.reschedule_outbox(root.accountId, root.rescheduleId, sendAt);
            root.rescheduleId = -1;
            root.changed(r !== "" ? r : qsTr("Rescheduled"));
        }
    }

    Dialog {
        id: deleteConfirm
        title: qsTr("Delete unsent mail?")
        modal: true
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(420, (parent ? parent.width : 420) - 16)
        padding: Theme.lg

        property double rowId: -1
        property string subject: ""

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        contentItem: Label {
            wrapMode: Text.Wrap
            color: Theme.text
            text: qsTr("“%1” will not be sent and cannot be recovered.").arg(deleteConfirm.subject)
        }

        footer: RowLayout {
            spacing: Theme.sm
            Item {
                Layout.fillWidth: true
            }
            AppButton {
                Layout.bottomMargin: Theme.md
                text: qsTr("Cancel")
                onClicked: deleteConfirm.close()
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                text: qsTr("Delete")
                intent: "danger"
                onClicked: {
                    deleteConfirm.close();
                    root.deleteRow(deleteConfirm.rowId);
                }
            }
        }
    }
}
