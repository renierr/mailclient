import QtQuick
import QtQuick.Controls

import Mailclient
import "components"

// Message list. Expects `messages` ListModel with
// {uid, subject, from, date, snippet, unread, starred}.
//
// Selection is keyed on UID, never on the row index (flaw F1). The old code
// bound `ListView.currentIndex` to a property, then reassigned it from a
// click and re-emitted selection from `onCurrentIndexChanged`; every feed
// reload cleared the model, which reset currentIndex, which fired selection
// again and dragged the highlight back to row 0. Now clicks report a UID
// upwards, the highlight is derived from `currentUid`, and a model rebuild
// cannot move the selection.
Rectangle {
    id: root

    property var messages
    property int currentUid: -1
    property string folderName: ""
    property string filterText: ""

    signal messageSelected(int uid)
    signal starToggled(int uid)
    signal deleteRequested(int uid)

    color: Theme.bg

    // Visible rows after applying the search filter.
    function matches(m) {
        if (root.filterText === "")
            return true
        var q = root.filterText.toLowerCase()
        return (m.subject || "").toLowerCase().indexOf(q) !== -1
            || (m.from || "").toLowerCase().indexOf(q) !== -1
            || (m.snippet || "").toLowerCase().indexOf(q) !== -1
    }

    function rebuildFiltered() {
        filtered.clear()
        if (!root.messages)
            return
        for (var i = 0; i < root.messages.count; i++) {
            var m = root.messages.get(i)
            if (root.matches(m))
                filtered.append(m)
        }
    }

    function indexOfUid(uid) {
        for (var i = 0; i < filtered.count; i++) {
            if (filtered.get(i).uid === uid)
                return i
        }
        return -1
    }

    // Move selection by one row (keyboard navigation from Main).
    function step(delta) {
        if (filtered.count === 0)
            return
        var i = root.indexOfUid(root.currentUid)
        var next = i < 0 ? 0 : Math.max(0, Math.min(filtered.count - 1, i + delta))
        root.messageSelected(filtered.get(next).uid)
        list.positionViewAtIndex(next, ListView.Contain)
    }

    ListModel {
        id: filtered
    }

    onMessagesChanged: root.rebuildFiltered()
    onFilterTextChanged: root.rebuildFiltered()

    Connections {
        target: root.messages
        // The feed is replaced wholesale on every reload; mirror it.
        function onCountChanged() { root.rebuildFiltered() }
    }

    Column {
        anchors.fill: parent

        // Header: which folder, how many, and what the filter narrowed it to.
        Rectangle {
            width: parent.width
            height: 38
            color: Theme.bgAlt
            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: 1
                color: Theme.border
            }
            Label {
                anchors.verticalCenter: parent.verticalCenter
                anchors.left: parent.left
                anchors.leftMargin: Theme.md
                text: root.folderName === "" ? qsTr("Messages") : root.folderName
                color: Theme.text
                font.pixelSize: Theme.fontBase
                font.bold: true
                elide: Text.ElideRight
                width: parent.width - 110
            }
            Label {
                anchors.verticalCenter: parent.verticalCenter
                anchors.right: parent.right
                anchors.rightMargin: Theme.md
                text: root.filterText === ""
                      ? qsTr("%1").arg(filtered.count)
                      : qsTr("%1 of %2").arg(filtered.count).arg(root.messages ? root.messages.count : 0)
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
            }
        }

        ListView {
            id: list
            width: parent.width
            height: parent.height - 38
            clip: true
            model: filtered
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

            delegate: Item {
                id: row
                width: list.width
                height: Theme.listItemHeight

                required property int index
                required property var model

                readonly property bool current: row.model.uid === root.currentUid

                Rectangle {
                    anchors.fill: parent
                    color: row.current ? Theme.selected
                         : hoverArea.containsMouse ? Theme.hover
                         : "transparent"

                    // Accent bar marks the selected row without relying on
                    // ListView.isCurrentItem (which is only valid on the
                    // delegate root and silently did nothing in children).
                    Rectangle {
                        width: 3
                        height: parent.height
                        color: Theme.accent
                        visible: row.current
                    }
                    Rectangle {
                        anchors.bottom: parent.bottom
                        width: parent.width
                        height: 1
                        color: Theme.border
                        opacity: 0.6
                    }
                }

                MouseArea {
                    id: hoverArea
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    onClicked: mouse => {
                        if (mouse.button === Qt.RightButton)
                            rowMenu.popup()
                        else
                            root.messageSelected(row.model.uid)
                    }
                }

                Menu {
                    id: rowMenu
                    MenuItem {
                        text: row.model.starred ? qsTr("Remove star") : qsTr("Star")
                        onTriggered: root.starToggled(row.model.uid)
                    }
                    MenuItem {
                        text: qsTr("Delete")
                        onTriggered: root.deleteRequested(row.model.uid)
                    }
                }

                Row {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.md
                    anchors.rightMargin: Theme.sm
                    anchors.topMargin: Theme.sm
                    anchors.bottomMargin: Theme.sm
                    spacing: Theme.sm

                    // Unread marker column.
                    Item {
                        width: 8
                        height: parent.height
                        Rectangle {
                            anchors.centerIn: parent
                            width: 8
                            height: 8
                            radius: 4
                            color: Theme.accent
                            visible: row.model.unread
                        }
                    }

                    Avatar {
                        anchors.verticalCenter: parent.verticalCenter
                        seed: row.model.from || "?"
                        initials: (row.model.from || "?").replace(/^[^a-zA-Z0-9]*/, "").substring(0, 1).toUpperCase()
                    }

                    Column {
                        width: parent.width - 8 - 34 - (Theme.sm * 3) - 24
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 2

                        Row {
                            width: parent.width
                            spacing: Theme.sm
                            Label {
                                text: row.model.from || qsTr("(unknown sender)")
                                color: Theme.text
                                font.pixelSize: Theme.fontBase
                                font.bold: row.model.unread
                                elide: Text.ElideRight
                                width: parent.width - 66
                            }
                            Label {
                                text: row.model.date
                                color: Theme.textMuted
                                font.pixelSize: Theme.fontTiny
                                width: 58
                                horizontalAlignment: Text.AlignRight
                            }
                        }
                        Label {
                            text: row.model.subject
                            color: row.model.unread ? Theme.text : Theme.textMuted
                            font.pixelSize: Theme.fontBase
                            font.bold: row.model.unread
                            elide: Text.ElideRight
                            width: parent.width
                        }
                        Label {
                            text: row.model.snippet
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontSmall
                            elide: Text.ElideRight
                            width: parent.width
                        }
                    }

                    // Star, always visible when set, on hover otherwise.
                    IconButton {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 24
                        height: 24
                        fontSize: Theme.fontBase
                        visible: row.model.starred || hoverArea.containsMouse
                        text: row.model.starred ? "★" : "☆"
                        contentColor: row.model.starred ? Theme.star : Theme.textMuted
                        tooltip: row.model.starred ? qsTr("Remove star") : qsTr("Star")
                        onClicked: root.starToggled(row.model.uid)
                    }
                }
            }
        }
    }

    // Empty states, distinguishing "nothing here" from "nothing matched".
    Column {
        anchors.centerIn: parent
        spacing: Theme.sm
        visible: filtered.count === 0
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.filterText !== "" ? "🔍" : "📭"
            font.pixelSize: 32
            opacity: 0.5
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textMuted
            font.pixelSize: Theme.fontBase
            text: root.filterText !== "" ? qsTr("No message matches “%1”").arg(root.filterText)
                                         : qsTr("This folder is empty")
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            visible: root.filterText === ""
            text: qsTr("Press ⟳ to sync")
        }
    }
}
