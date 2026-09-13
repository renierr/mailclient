import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Mailclient

RowLayout {
    id: root

    property int selectedCount: 0
    property bool allStarred: false

    signal clearRequested()
    signal markReadRequested()
    signal markUnreadRequested()
    signal toggleStarRequested()
    signal archiveRequested()
    signal moveRequested()
    signal deleteRequested()
    signal moreRequested()

    height: Math.round(40 * Theme.uiScale)
    spacing: 2

    Label {
        Layout.leftMargin: Theme.md
        text: qsTr("%n selected", "", root.selectedCount)
        color: Theme.text
        font.pixelSize: Theme.fontSmall
        font.bold: true
    }

    IconButton {
        text: "✕"
        fontSize: Theme.fontSmall
        tooltip: qsTr("Clear selection")
        onClicked: root.clearRequested()
    }

    Item { Layout.fillWidth: true }

    IconButton {
        text: "✓"
        fontSize: Theme.fontSmall
        tooltip: qsTr("Mark selected as read")
        onClicked: root.markReadRequested()
    }

    IconButton {
        text: "○"
        fontSize: Theme.fontSmall
        tooltip: qsTr("Mark selected as unread")
        onClicked: root.markUnreadRequested()
    }

    IconButton {
        text: root.allStarred ? "☆" : "★"
        fontSize: Theme.fontBase
        contentColor: root.allStarred ? Theme.textMuted : Theme.star
        tooltip: root.allStarred ? qsTr("Remove star from selected") : qsTr("Star selected")
        onClicked: root.toggleStarRequested()
    }

    IconButton {
        text: "🗄"
        fontSize: Theme.fontSmall
        tooltip: qsTr("Archive selected")
        onClicked: root.archiveRequested()
    }

    IconButton {
        text: "➡"
        fontSize: Theme.fontSmall
        tooltip: qsTr("Move selected to…")
        onClicked: root.moveRequested()
    }

    IconButton {
        text: "🗑"
        fontSize: Theme.fontSmall
        tooltip: qsTr("Move selected to Trash")
        onClicked: root.deleteRequested()
    }

    IconButton {
        Layout.rightMargin: Theme.sm
        text: "⋯"
        fontSize: Theme.fontBase
        tooltip: qsTr("More bulk actions")
        onClicked: root.moreRequested()
    }
}
