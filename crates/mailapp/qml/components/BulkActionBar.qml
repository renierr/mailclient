import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Mailclient

RowLayout {
    id: root

    property int selectedCount: 0
    property bool allStarred: false
    // Deleting the selection destroys it (core `delete_prompt`): the Trash
    // button becomes Delete permanently.
    property bool deletePermanent: false

    signal clearRequested
    signal markReadRequested
    signal markUnreadRequested
    signal toggleStarRequested
    signal archiveRequested
    signal moveRequested
    signal deleteRequested
    signal moreRequested

    height: Math.round(40 * Theme.uiScale)
    spacing: 2
    clip: true

    // The common actions lead (Archive, Trash). Narrow panes cannot fit the
    // whole row: collapse the least-critical buttons first so Archive,
    // Delete and the overflow menu stay reachable. Anything hidden here
    // lives on in the overflow menu (MessageList.bulkMenu).
    readonly property bool compact: root.width > 0 && root.width < Math.round(380 * Theme.uiScale)
    readonly property bool veryCompact: root.width > 0 && root.width < Math.round(300 * Theme.uiScale)

    Label {
        Layout.leftMargin: Theme.md
        Layout.minimumWidth: 0
        Layout.preferredWidth: Math.min(implicitWidth, Math.max(Math.round(28 * Theme.uiScale), root.width - Math.round(
                                                                    220 * Theme.uiScale)))
        text: root.veryCompact ? qsTr("%1").arg(root.selectedCount) : qsTr("%n selected", "", root.selectedCount)
        color: Theme.text
        font.pixelSize: Theme.fontSmall
        font.bold: true
        elide: Text.ElideRight
    }

    IconButton {
        text: Icons.close
        iconFont: true
        fontSize: Theme.fontSmall
        tooltip: qsTr("Clear selection")
        onClicked: root.clearRequested()
    }

    Item {
        Layout.fillWidth: true
    }

    IconButton {
        text: Icons.archive
        iconFont: true
        fontSize: Theme.fontSmall
        tooltip: qsTr("Archive selected")
        onClicked: root.archiveRequested()
    }

    IconButton {
        text: root.deletePermanent ? Icons.deleteForever : Icons.trash
        iconFont: true
        fontSize: Theme.fontSmall
        tooltip: root.deletePermanent ? qsTr("Delete selected permanently") : qsTr("Move selected to Trash")
        onClicked: root.deleteRequested()
    }

    IconButton {
        visible: !root.compact
        text: Icons.driveFileMove
        iconFont: true
        fontSize: Theme.fontSmall
        tooltip: qsTr("Move selected to…")
        onClicked: root.moveRequested()
    }

    IconButton {
        visible: !root.veryCompact
        text: Icons.done
        iconFont: true
        fontSize: Theme.fontSmall
        tooltip: qsTr("Mark selected as read")
        onClicked: root.markReadRequested()
    }

    IconButton {
        visible: !root.veryCompact
        text: Icons.markUnread
        iconFont: true
        fontSize: Theme.fontSmall
        tooltip: qsTr("Mark selected as unread")
        onClicked: root.markUnreadRequested()
    }

    IconButton {
        visible: !root.compact
        text: root.allStarred ? Icons.starBorder : Icons.star
        iconFont: true
        fontSize: Theme.fontBase
        contentColor: root.allStarred ? Theme.textMuted : Theme.star
        tooltip: root.allStarred ? qsTr("Remove star from selected") : qsTr("Star selected")
        onClicked: root.toggleStarRequested()
    }

    IconButton {
        Layout.rightMargin: Theme.sm
        text: Icons.moreVert
        iconFont: true
        fontSize: Theme.fontBase
        tooltip: qsTr("More bulk actions")
        onClicked: root.moreRequested()
    }
}
