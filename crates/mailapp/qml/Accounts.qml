import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Account manager: list, switch, edit and remove accounts.
//
// Before this existed a mistyped account could only be added, never removed —
// exactly how a duplicate ends up in the sidebar with no way out.
Dialog {
    id: root
    title: qsTr("Accounts")
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 80 : 560, 560)
    height: Math.min(parent ? parent.height - 120 : 460, 460)
    padding: Theme.lg

    property var accounts
    property int currentAccountId: -1

    signal statusMessage(string text)
    signal addRequested()
    signal editRequested(int id)
    signal accountSelected(int id)
    signal deleteConfirmed(int id)

    // Pending delete, so the confirm dialog knows what it is confirming.
    property int pendingDeleteId: -1
    property string pendingDeleteEmail: ""

    background: Rectangle {
        color: Theme.bg
        radius: Theme.radiusLg
        border.width: 1
        border.color: Theme.border
    }

    header: Rectangle {
        implicitHeight: 48
        color: "transparent"
        Label {
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: Theme.lg
            text: root.title
            color: Theme.text
            font.pixelSize: Theme.fontMedium
            font.bold: true
        }
        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.border
        }
    }

    footer: RowLayout {
        spacing: Theme.sm
        Button {
            Layout.leftMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            text: qsTr("Add account…")
            onClicked: root.addRequested()
        }
        Item { Layout.fillWidth: true }
        Button {
            Layout.rightMargin: Theme.lg
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
            height: 66
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
                    implicitWidth: 32
                    implicitHeight: 32
                    seed: accountRow.model.email
                    initials: (accountRow.model.email || "?").substring(0, 1).toUpperCase()
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    RowLayout {
                        spacing: Theme.sm
                        Label {
                            text: accountRow.model.email
                            color: Theme.text
                            font.pixelSize: Theme.fontBase
                            font.bold: true
                            elide: Text.ElideRight
                        }
                        Rectangle {
                            visible: accountRow.current
                            implicitWidth: activeLabel.implicitWidth + Theme.sm
                            implicitHeight: 16
                            radius: 8
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
                    }
                }

                IconButton {
                    text: "✓"
                    tooltip: qsTr("Use this account")
                    enabled: !accountRow.current
                    onClicked: root.accountSelected(accountRow.model.id)
                }
                IconButton {
                    text: "✎"
                    tooltip: qsTr("Edit")
                    onClicked: root.editRequested(accountRow.model.id)
                }
                IconButton {
                    text: "🗑"
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
        standardButtons: Dialog.Cancel | Dialog.Yes

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        onAccepted: {
            root.deleteConfirmed(root.pendingDeleteId)
            root.pendingDeleteId = -1
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
