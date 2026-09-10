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
Dialog {
    id: root
    title: qsTr("IMAP folders")
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 80 : 560, 560)
    height: Math.min(parent ? parent.height - 120 : 520, 520)
    padding: Theme.lg

    property var folders
    property string currentFolder: ""
    property bool busy: false

    signal statusMessage(string text)
    signal refreshRequested()
    signal visibilityToggled(string path, bool subscribed)
    signal folderSelected(string path)

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
        AppButton {
            Layout.leftMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            text: root.busy ? qsTr("Refreshing…") : qsTr("Refresh from server")
            enabled: !root.busy
            onClicked: root.refreshRequested()
        }
        Item { Layout.fillWidth: true }
        AppButton {
            Layout.rightMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            text: qsTr("Close")
            onClicked: root.close()
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Theme.sm

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
                height: 44
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
                        spacing: 0
                        Label {
                            text: folderRow.model.name
                            color: Theme.text
                            font.pixelSize: Theme.fontBase
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        Label {
                            text: qsTr("%1 cached · %2 unread")
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
