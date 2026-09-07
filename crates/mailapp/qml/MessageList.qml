import QtQuick
import QtQuick.Controls

import "components"

// Message list. Expects `messages` ListModel with
// {subject, from, date, snippet, unread, starred}.
// Emits messageSelected(index).
Pane {
    id: root

    property var messages
    property int currentIndex: 0
    signal messageSelected(int index)

    padding: 0

    ListView {
        id: list
        anchors.fill: parent
        clip: true
        model: root.messages
        currentIndex: root.currentIndex
        onCurrentIndexChanged: root.messageSelected(currentIndex)
        delegate: Item {
            width: list.width
            height: 84
            Rectangle {
                anchors.fill: parent
                color: ListView.isCurrentItem ? palette.highlight : "transparent"
            }
            MouseArea {
                anchors.fill: parent
                onClicked: list.currentIndex = index
            }
            Row {
                anchors.fill: parent
                anchors.margins: 8
                spacing: 8
                Avatar {
                    initials: (model.from || "?").substring(0, 1).toUpperCase()
                    anchors.verticalCenter: parent.verticalCenter
                }
                Column {
                    width: parent.width - 52
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 2
                    Row {
                        width: parent.width
                        Label {
                            text: model.from
                            font.bold: model.unread
                            elide: Text.ElideRight
                            width: parent.width - 70
                            color: ListView.isCurrentItem ? palette.highlightedText : palette.text
                        }
                        Label {
                            text: model.date
                            opacity: 0.6
                            width: 66
                            horizontalAlignment: Text.AlignRight
                            font.pixelSize: 11
                            color: ListView.isCurrentItem ? palette.highlightedText : palette.text
                        }
                    }
                    Label {
                        text: (model.starred ? "★ " : "") + model.subject
                        font.bold: model.unread
                        elide: Text.ElideRight
                        width: parent.width
                        color: ListView.isCurrentItem ? palette.highlightedText : palette.text
                    }
                    Label {
                        text: model.snippet
                        opacity: 0.6
                        elide: Text.ElideRight
                        width: parent.width
                        font.pixelSize: 12
                        color: ListView.isCurrentItem ? palette.highlightedText : palette.text
                    }
                }
            }
        }
    }
}
