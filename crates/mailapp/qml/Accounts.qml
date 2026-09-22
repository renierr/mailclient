import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Account manager: list, switch, edit and remove accounts.
//
// Before this existed a mistyped account could only be added, never removed —
// exactly how a duplicate ends up in the sidebar with no way out.
AppDialog {
    id: root
    title: qsTr("Accounts")
    preferredWidth: 560
    preferredHeight: 460
    minWidth: 400
    minHeight: 300
    padding: Theme.lg

    property var accounts
    property int currentAccountId: -1

    signal statusMessage(string text)
    signal addRequested()
    signal editRequested(int id)
    signal accountSelected(int id)
    signal deleteConfirmed(int id)

    // These rebuild the account model, destroying the row that was clicked.
    function emitLater(sig, arg) {
        if (arg === undefined)
            Qt.callLater(sig)
        else
            Qt.callLater(sig, arg)
    }

    // Pending delete, so the confirm dialog knows what it is confirming.
    property int pendingDeleteId: -1
    property string pendingDeleteEmail: ""

    footer: RowLayout {
        spacing: Theme.sm
        AppButton {
            Layout.leftMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            text: qsTr("Add account…")
            onClicked: root.addRequested()
        }
        Item { Layout.fillWidth: true }
        AppButton {
            Layout.rightMargin: Theme.lg + 8
            Layout.bottomMargin: Theme.md
            text: qsTr("Close")
            onClicked: root.close()
        }
    }

    ListView {
        id: accountList
        anchors.fill: parent
        clip: true
        spacing: Theme.sm
        model: root.accounts
        boundsBehavior: Flickable.StopAtBounds
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        delegate: Rectangle {
            id: accountRow
            width: accountList.width
            height: Math.round(66 * Theme.uiScale)
            radius: Theme.radius
            color: current ? Theme.selected : Theme.bgAlt
            border.width: 1
            border.color: current ? Theme.accent : Theme.border

            required property var model
            readonly property bool current: accountRow.model.id === root.currentAccountId

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Theme.md
                anchors.rightMargin: Theme.sm
                spacing: Theme.md

                Avatar {
                    implicitWidth: Math.round(32 * Theme.uiScale)
                    implicitHeight: Math.round(32 * Theme.uiScale)
                    seed: accountRow.model.email
                    initials: (accountRow.model.email || "?").substring(0, 1).toUpperCase()
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    spacing: 0
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        spacing: Theme.sm
                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            text: accountRow.model.email
                            color: Theme.text
                            font.pixelSize: Theme.fontBase
                            font.bold: true
                            elide: Text.ElideRight
                        }
                        Rectangle {
                            visible: accountRow.current
                            implicitWidth: activeLabel.implicitWidth + Theme.sm
                            implicitHeight: Math.round(16 * Theme.uiScale)
                            radius: Math.round(8 * Theme.uiScale)
                            color: Theme.accent
                            Label {
                                id: activeLabel
                                anchors.centerIn: parent
                                text: qsTr("active")
                                color: Theme.accentText
                                font.pixelSize: Theme.fontTiny
                            }
                        }
                    }
                    Label {
                        text: qsTr("%1 · IMAP %2:%3")
                              .arg(accountRow.model.name)
                              .arg(accountRow.model.imap_host)
                              .arg(accountRow.model.imap_port)
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontTiny
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                    }
                }

                IconButton {
                    text: Icons.done
                    iconFont: true
                    tooltip: qsTr("Use this account")
                    enabled: !accountRow.current
                    onClicked: root.emitLater(root.accountSelected, accountRow.model.id)
                }
                IconButton {
                    text: Icons.edit
                    iconFont: true
                    tooltip: qsTr("Edit")
                    onClicked: root.emitLater(root.editRequested, accountRow.model.id)
                }
                IconButton {
                    text: Icons.trash
                    iconFont: true
                    tooltip: qsTr("Remove")
                    contentColor: Theme.danger
                    onClicked: {
                        root.pendingDeleteId = accountRow.model.id
                        root.pendingDeleteEmail = accountRow.model.email
                        confirmDelete.open()
                    }
                }
            }
        }
    }

    Label {
        anchors.centerIn: parent
        visible: !root.accounts || root.accounts.count === 0
        text: qsTr("No accounts yet.")
        color: Theme.textMuted
        font.pixelSize: Theme.fontBase
    }

    // Removing an account drops its cached mail and its keyring secret, so it
    // asks first and says exactly what will happen.
    Dialog {
        id: confirmDelete
        title: qsTr("Remove account?")
        modal: true
        anchors.centerIn: parent
        width: 420
        padding: Theme.lg

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        // Explicit footer instead of standardButtons: those are drawn by the
        // Controls style, so they would not match any other button here.
        footer: RowLayout {
            spacing: Theme.sm
            Item { Layout.fillWidth: true }
            AppButton {
                text: qsTr("Cancel")
                onClicked: confirmDelete.close()
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                Layout.topMargin: Theme.sm
                text: qsTr("Remove")
                intent: "danger"
                onClicked: {
                    root.deleteConfirmed(root.pendingDeleteId)
                    root.pendingDeleteId = -1
                    confirmDelete.close()
                }
            }
        }

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            color: Theme.text
            font.pixelSize: Theme.fontBase
            text: qsTr("Remove %1?\n\nIts cached folders and messages are deleted locally and its password is removed from the OS keyring. Mail on the server is untouched.")
                  .arg(root.pendingDeleteEmail)
        }
    }
}
