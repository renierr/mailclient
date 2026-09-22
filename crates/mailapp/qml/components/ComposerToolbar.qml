import QtQuick
import QtQuick.Layouts

import Mailclient

// Formatting toolbar for the composer: bold, italic, underline, list,
// quote, link, attach, and HTML source toggle.
Rectangle {
    id: root

    property bool sourceMode: false
    property var bodyEditor
    signal linkRequested()
    signal attachRequested()
    signal toggleSourceRequested()

    implicitHeight: Math.round(38 * Theme.uiScale)
    radius: Theme.radius
    color: Theme.bgAlt
    border.width: 1
    border.color: Theme.border

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: Theme.xs
        anchors.rightMargin: Theme.xs
        spacing: 2

        IconButton {
            text: "B"
            tooltip: qsTr("Bold (Ctrl+B)")
            enabled: !root.sourceMode
            active: root.bodyEditor ? root.bodyEditor.boldActive : false
            font.bold: true
            onClicked: if (root.bodyEditor) root.bodyEditor.exec("bold")
        }
        IconButton {
            text: "I"
            tooltip: qsTr("Italic (Ctrl+I)")
            enabled: !root.sourceMode
            active: root.bodyEditor ? root.bodyEditor.italicActive : false
            font.italic: true
            onClicked: if (root.bodyEditor) root.bodyEditor.exec("italic")
        }
        IconButton {
            text: "U"
            tooltip: qsTr("Underline (Ctrl+U)")
            enabled: !root.sourceMode
            active: root.bodyEditor ? root.bodyEditor.underlineActive : false
            font.underline: true
            onClicked: if (root.bodyEditor) root.bodyEditor.exec("underline")
        }

        Rectangle {
            implicitWidth: 1
            implicitHeight: 20
            color: Theme.border
        }

        IconButton {
            text: Icons.formatListBulleted
            iconFont: true
            tooltip: qsTr("Bullet list")
            enabled: !root.sourceMode
            active: root.bodyEditor ? root.bodyEditor.listActive : false
            onClicked: if (root.bodyEditor) root.bodyEditor.exec("insertUnorderedList")
        }
        IconButton {
            text: Icons.formatQuote
            iconFont: true
            tooltip: qsTr("Quote")
            enabled: !root.sourceMode
            active: root.bodyEditor ? root.bodyEditor.quoteActive : false
            onClicked: if (root.bodyEditor) root.bodyEditor.exec("formatBlock", root.bodyEditor.quoteActive ? "p" : "blockquote")
        }
        IconButton {
            text: Icons.link
            iconFont: true
            tooltip: qsTr("Insert link")
            enabled: !root.sourceMode
            onClicked: root.linkRequested()
        }
        IconButton {
            text: Icons.close
            iconFont: true
            tooltip: qsTr("Clear formatting")
            enabled: !root.sourceMode
            onClicked: if (root.bodyEditor) root.bodyEditor.exec("removeFormat")
        }
        IconButton {
            text: Icons.attachFile
            iconFont: true
            tooltip: qsTr("Attach files")
            onClicked: root.attachRequested()
        }

        Item { Layout.fillWidth: true }

        IconButton {
            text: "</>"
            fontSize: Theme.fontSmall
            implicitWidth: Math.round(40 * Theme.uiScale)
            tooltip: qsTr("Toggle HTML source")
            active: root.sourceMode
            onClicked: root.toggleSourceRequested()
        }
    }
}
