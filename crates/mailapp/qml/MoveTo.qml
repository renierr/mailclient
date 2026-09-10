import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Move-to picker: choose any visible folder (subfolders indented by depth)
// as the destination for one message. Only subscribed folders are listed —
// unhide the rest in the Folders manager first.
Dialog {
    id: root
    title: qsTr("Move to…")
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 80 : 480, 480)
    height: Math.min(parent ? parent.height - 120 : 520, 520)
    padding: Theme.lg

    property var folders
    property string currentFolder: ""
    property int uid: -1
    property string subject: ""

    signal folderChosen(string path)

    function emitLater(sig, arg) {
        Qt.callLater(sig, arg)
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

    // Hierarchy depth from the stored path + delimiter (subfolders indent).
    function depthOf(model) {
        var delim = model.delimiter !== undefined && model.delimiter !== "" ? model.delimiter : "/"
        return model.name.split(delim).length - 1
    }

    function shortName(model) {
        var delim = model.delimiter !== undefined && model.delimiter !== "" ? model.delimiter : "/"
        var parts = model.name.split(delim)
        return parts[parts.length - 1]
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
        Item { Layout.fillWidth: true }
        AppButton {
            Layout.rightMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            text: qsTr("Cancel")
            onClicked: root.close()
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Theme.sm

        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            elide: Text.ElideRight
            maximumLineCount: 2
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            text: root.subject !== "" ? qsTr("Move “%1” to:").arg(root.subject) : qsTr("Move to:")
        }

        ListView {
            id: folderList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            spacing: 2
            // Set inline (not via a binding loop): subscribed-only subset.
            model: ListModel { id: shown }
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

            Component.onCompleted: root.refreshShown()

            delegate: ItemDelegate {
                id: folderRow
                width: folderList.width
                height: Theme.rowHeight

                required property var model
                readonly property bool current: folderRow.model.name === root.currentFolder
                enabled: !folderRow.current

                leftPadding: Theme.md + Theme.sm + root.depthOf(folderRow.model) * 16
                rightPadding: Theme.md + Theme.sm
                onClicked: root.emitLater(root.folderChosen, folderRow.model.name)

                background: Rectangle {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.sm
                    anchors.rightMargin: Theme.sm
                    radius: Theme.radius
                    color: folderRow.hovered && !folderRow.current ? Theme.hover : "transparent"
                    opacity: folderRow.current ? 0.4 : 1.0
                }

                contentItem: RowLayout {
                    spacing: Theme.sm
                    Label {
                        text: root.roleIcon(folderRow.model.role)
                        font.pixelSize: Theme.fontBase
                    }
                    Label {
                        text: root.shortName(folderRow.model)
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                        color: folderRow.current ? Theme.textMuted : Theme.text
                        font.pixelSize: Theme.fontBase
                    }
                }
            }
        }
    }

    function refreshShown() {
        var rows = []
        if (root.folders) {
            for (var i = 0; i < root.folders.count; i++) {
                var f = root.folders.get(i)
                if (f.subscribed === false)
                    continue
                rows.push({
                    name: f.name,
                    role: f.role,
                    delimiter: f.delimiter !== undefined ? f.delimiter : "/"
                })
            }
        }
        ModelSync.sync(shown, rows, "name")
    }

    onFoldersChanged: root.refreshShown()
    onOpened: root.refreshShown()
}
