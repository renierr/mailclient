import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Folder/account sidebar. Expects `folders` ListModel with
// {name, role, unread, subscribed, count} and `accounts` ListModel with
// {id, name, email}. Only subscribed (visible) folders are listed — the rest
// live in the Folders manager.
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
    signal manageFoldersRequested()
    signal addAccountRequested()

    // Visible subset of `folders` (subscribed !== false). Kept as its own
    // model so hiding a folder never destroys the full feed the manager
    // dialog reads — same in-place update discipline as ModelSync.
    ListModel { id: shown }

    function refreshShown() {
        var rows = []
        if (root.folders) {
            for (var i = 0; i < root.folders.count; i++) {
                var f = root.folders.get(i)
                if (f.subscribed === false)
                    continue
                rows.push({ name: f.name, role: f.role, unread: f.unread })
            }
        }
        ModelSync.sync(shown, rows, "name")
    }

    onFoldersChanged: root.refreshShown()

    // Switching account or folder rebuilds the models these delegates and the
    // account popup are built from, so the emit is deferred out of the click
    // handler (see MessageList.emitLater for the crash this avoids).
    function emitLater(sig, arg) {
        if (arg === undefined)
            Qt.callLater(sig)
        else
            Qt.callLater(sig, arg)
    }

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

            AppMenu {
                id: accountMenu
                Repeater {
                    model: root.accounts
                    MenuItem {
                        required property var model
                        text: (model.id === root.currentAccountId ? "● " : "   ") + model.email
                        onTriggered: root.emitLater(root.accountSelected, model.id)
                    }
                }
                MenuSeparator {}
                MenuItem {
                    text: qsTr("Add account…")
                    onTriggered: root.emitLater(root.addAccountRequested)
                }
                MenuItem {
                    text: qsTr("Manage accounts…")
                    onTriggered: root.emitLater(root.manageAccountsRequested)
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Theme.md
            Layout.rightMargin: Theme.sm
            Layout.topMargin: Theme.xs
            Layout.bottomMargin: Theme.xs
            spacing: Theme.xs

            Label {
                Layout.fillWidth: true
                text: qsTr("FOLDERS")
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
                font.bold: true
                font.letterSpacing: 1
            }

            IconButton {
                text: "⛭"
                tooltip: qsTr("Manage IMAP folders…")
                onClicked: root.manageFoldersRequested()
            }
        }

        ListView {
            id: folderList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: shown
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

            delegate: ItemDelegate {
                id: folderRow
                width: folderList.width
                height: Theme.rowHeight

                required property var model
                readonly property bool current: folderRow.model.name === root.currentFolder

                onClicked: root.emitLater(root.folderSelected, folderRow.model.name)
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
            visible: shown.count === 0
            text: root.currentEmail === "" ? qsTr("Add an account to begin.")
                                            : qsTr("No folders yet — press ⟳ to sync.")
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.Wrap
        }
    }
}
