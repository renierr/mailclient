import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Folder/account sidebar. Expects `folders` ListModel with
// {name, role, unread} and `accounts` ListModel with {id, name, email}.
Rectangle {
    id: root

    property var folders
    property var accounts
    property string currentFolder: ""
    property string currentEmail: ""
    property int currentAccountId: -1

    signal folderSelected(string path)
    signal accountSelected(int id)
    signal manageAccountsRequested()
    signal addAccountRequested()

    color: Theme.bgAlt

    Rectangle {
        anchors.right: parent.right
        width: 1
        height: parent.height
        color: Theme.border
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Account chip: shows who you are, opens the account menu.
        ItemDelegate {
            id: accountChip
            Layout.fillWidth: true
            Layout.margins: Theme.sm
            implicitHeight: 48
            onClicked: accountMenu.popup()

            background: Rectangle {
                radius: Theme.radius
                color: accountChip.hovered ? Theme.hover : "transparent"
                border.width: 1
                border.color: Theme.border
            }

            contentItem: RowLayout {
                spacing: Theme.sm
                Avatar {
                    implicitWidth: 28
                    implicitHeight: 28
                    seed: root.currentEmail
                    initials: (root.currentEmail || "?").substring(0, 1).toUpperCase()
                }
                ColumnLayout {
                    spacing: 0
                    Layout.fillWidth: true
                    Label {
                        text: root.currentEmail === "" ? qsTr("No account") : root.currentEmail
                        color: Theme.text
                        font.pixelSize: Theme.fontSmall
                        font.bold: true
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    Label {
                        text: root.accounts && root.accounts.count > 1
                              ? qsTr("%1 accounts — switch").arg(root.accounts.count)
                              : qsTr("Manage account")
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontTiny
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }
                Label {
                    text: "⌄"
                    color: Theme.textMuted
                }
            }

            Menu {
                id: accountMenu
                Repeater {
                    model: root.accounts
                    MenuItem {
                        required property var model
                        text: (model.id === root.currentAccountId ? "● " : "   ") + model.email
                        onTriggered: root.accountSelected(model.id)
                    }
                }
                MenuSeparator {}
                MenuItem {
                    text: qsTr("Add account…")
                    onTriggered: root.addAccountRequested()
                }
                MenuItem {
                    text: qsTr("Manage accounts…")
                    onTriggered: root.manageAccountsRequested()
                }
            }
        }

        Label {
            Layout.fillWidth: true
            Layout.leftMargin: Theme.md
            Layout.topMargin: Theme.xs
            Layout.bottomMargin: Theme.xs
            text: qsTr("FOLDERS")
            color: Theme.textMuted
            font.pixelSize: Theme.fontTiny
            font.bold: true
            font.letterSpacing: 1
        }

        ListView {
            id: folderList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.folders
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

            delegate: ItemDelegate {
                id: folderRow
                width: folderList.width
                height: Theme.rowHeight

                required property var model
                readonly property bool current: folderRow.model.name === root.currentFolder

                onClicked: root.folderSelected(folderRow.model.name)
                // Padding, not anchors: a control's contentItem is sized by
                // the control, so anchor margins inside it are ignored.
                leftPadding: Theme.md + Theme.sm
                rightPadding: Theme.md + Theme.sm

                background: Rectangle {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.sm
                    anchors.rightMargin: Theme.sm
                    radius: Theme.radius
                    color: folderRow.current ? Theme.selected
                         : folderRow.hovered ? Theme.hover
                         : "transparent"
                }

                contentItem: RowLayout {
                    spacing: Theme.sm

                    Label {
                        text: {
                            switch (folderRow.model.role) {
                            case "inbox": return "📥"
                            case "drafts": return "📝"
                            case "sent": return "📤"
                            case "archive": return "🗄"
                            case "junk": return "🚫"
                            case "trash": return "🗑"
                            default: return "📁"
                            }
                        }
                        font.pixelSize: Theme.fontBase
                    }
                    Label {
                        text: folderRow.model.name
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                        color: folderRow.current ? Theme.accent : Theme.text
                        font.pixelSize: Theme.fontBase
                        font.bold: folderRow.model.unread > 0
                    }
                    // Unread count as a pill, the way mail clients do it.
                    Rectangle {
                        visible: folderRow.model.unread > 0
                        implicitWidth: Math.max(20, unreadLabel.implicitWidth + Theme.sm)
                        implicitHeight: 18
                        radius: 9
                        color: folderRow.current ? Theme.accent : Theme.border
                        Label {
                            id: unreadLabel
                            anchors.centerIn: parent
                            text: folderRow.model.unread
                            color: folderRow.current ? Theme.accentText : Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                            font.bold: true
                        }
                    }
                }
            }
        }

        // No folders yet: say what to do about it.
        Label {
            Layout.fillWidth: true
            Layout.margins: Theme.md
            visible: !root.folders || root.folders.count === 0
            text: root.currentEmail === "" ? qsTr("Add an account to begin.")
                                           : qsTr("No folders yet — press ⟳ to sync.")
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.Wrap
        }
    }
}
