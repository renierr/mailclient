import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// IMAP folder manager: where folders come from and which to view.
//
// Folders arrive via LIST on every sync (`refreshRequested` re-runs just the
// LIST, no message bodies). Each row toggles sidebar visibility
// (`visibilityToggled` flips the `subscribed` flag — display-only, the cache
// stays and auto-sync skips hidden folders). Opening a row jumps to it.
AppDialog {
    id: root
    title: qsTr("IMAP folders")
    preferredWidth: 560
    preferredHeight: 520
    minWidth: 400
    minHeight: 320
    padding: Theme.lg

    property var folders
    property string currentFolder: ""
    property bool busy: false

    signal statusMessage(string text)
    signal refreshRequested()
    signal visibilityToggled(string path, bool subscribed)
    signal folderSelected(string path)
    signal createRequested(string path)

    function clearNewFolder() {
        newFolderField.text = ""
    }

    function emitLater(sig, arg1, arg2) {
        if (arg1 === undefined)
            Qt.callLater(sig)
        else if (arg2 === undefined)
            Qt.callLater(sig, arg1)
        else
            Qt.callLater(sig, arg1, arg2)
    }

    function roleIcon(role) {
        switch (role) {
        case "inbox": return "📥"
        case "drafts": return "📝"
        case "sent": return "📤"
        case "archive": return "🗄"
        case "junk": return "🚫"
        case "trash": return "🗑"
        default: return "📁"
        }
    }

    footer: RowLayout {
        spacing: Theme.sm
        AppButton {
            Layout.leftMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            text: root.busy ? qsTr("Refreshing…") : qsTr("Refresh from server")
            enabled: !root.busy
            onClicked: root.refreshRequested()
        }
        Item { Layout.fillWidth: true }
        AppButton {
            Layout.rightMargin: Theme.lg + 8
            Layout.bottomMargin: Theme.md
            text: qsTr("Close")
            onClicked: root.close()
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Theme.sm

        // Create a folder server-side (`/` separates levels: `Work/Client`).
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.sm
            AppTextField {
                id: newFolderField
                Layout.fillWidth: true
                placeholderText: qsTr("New folder name… (`/` for subfolders)")
                onAccepted: root.emitLater(root.createRequested, newFolderField.text)
            }
            AppButton {
                text: qsTr("Create")
                intent: "primary"
                enabled: newFolderField.text.trim() !== ""
                onClicked: root.createRequested(newFolderField.text)
            }
        }

        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            text: qsTr("Uncheck to hide a folder from the sidebar. Hidden folders keep their cached mail and skip auto-sync; opening one still syncs it.")
        }

        ListView {
            id: folderList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 2
            model: root.folders
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

            delegate: Rectangle {
                id: folderRow
                width: folderList.width
                height: Math.round(44 * Theme.uiScale)
                radius: Theme.radius
                color: folderRow.model.name === root.currentFolder ? Theme.selected : "transparent"

                required property var model

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.sm
                    anchors.rightMargin: Theme.sm
                    spacing: Theme.sm

                    AppCheckBox {
                        checked: folderRow.model.subscribed !== false
                        onToggled: root.emitLater(root.visibilityToggled, folderRow.model.name, checked)
                    }

                    Label {
                        text: root.roleIcon(folderRow.model.role)
                        font.pixelSize: Theme.fontBase
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        spacing: 0
                        Label {
                            text: folderRow.model.name
                            color: Theme.text
                            font.pixelSize: Theme.fontBase
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        Label {
                            text: qsTr("%1 total · %2 unread")
                                  .arg(folderRow.model.count !== undefined ? folderRow.model.count : 0)
                                  .arg(folderRow.model.unread)
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                    }

                    IconButton {
                        text: "→"
                        tooltip: qsTr("Open folder")
                        onClicked: root.emitLater(root.folderSelected, folderRow.model.name)
                    }
                }
            }
        }

        Label {
            Layout.fillWidth: true
            visible: !root.folders || root.folders.count === 0
            horizontalAlignment: Text.AlignHCenter
            text: qsTr("No folders yet — press “Refresh from server”.")
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
        }
    }
}
