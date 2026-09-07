import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtWebEngine

// Reader pane. `message` is a ListElement-like object with
// {subject, from, date, body}. HTML bodies render in a sandboxed
// WebEngine view (JS/plugins off, remote images per setting);
// plain text uses RichText.
Pane {
    id: root

    property var message
    property bool loadRemoteImages: false
    signal replyRequested()
    signal forwardRequested()
    signal starRequested()
    signal deleteRequested()
    signal statusMessage(string text)

    padding: 8

    property bool isHtml: false
    property string htmlBody: ""

    function looksLikeHtml(t) {
        return t.indexOf("<") !== -1 && t.indexOf(">") !== -1
    }

    onMessageChanged: {
        var b = root.message ? root.message.body : ""
        if (looksLikeHtml(b)) {
            root.isHtml = true
            root.htmlBody = b
        } else {
            root.isHtml = false
        }
    }

    onHtmlBodyChanged: {
        if (bodyLoader.item)
            bodyLoader.item.loadHtml(root.htmlBody, "")
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 8
        visible: root.message !== undefined

        RowLayout {
            Layout.fillWidth: true
            Label {
                Layout.fillWidth: true
                text: root.message ? root.message.subject : ""
                font.pixelSize: 18
                font.bold: true
                wrapMode: Text.Wrap
            }
            ToolButton {
                text: qsTr("↩ Reply")
                onClicked: root.replyRequested()
            }
            ToolButton {
                text: qsTr("→ Forward")
                onClicked: root.forwardRequested()
            }
            ToolButton {
                text: qsTr("🗑")
                Accessible.name: qsTr("Delete")
                onClicked: root.deleteRequested()
            }
            ToolButton {
                text: qsTr("★")
                Accessible.name: qsTr("Star")
                onClicked: root.starRequested()
            }
        }

        Label {
            text: root.message ? qsTr("From: %1   •   %2").arg(root.message.from).arg(root.message.date) : ""
            opacity: 0.7
            font.pixelSize: 12
        }
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            opacity: 0.2
            color: palette.text
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: !root.isHtml
            Text {
                width: root.width - 32
                text: root.message ? root.message.body : ""
                textFormat: Text.RichText
                wrapMode: Text.Wrap
                // Base color for text without explicit colors (theme-aware).
                color: palette.text
                linkColor: palette.link
                onLinkActivated: link => root.statusMessage(qsTr("Blocked remote link (M3 sandbox): %1").arg(link))
            }
        }

        Loader {
            id: bodyLoader
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: root.isHtml
            active: root.message !== undefined && root.isHtml
            sourceComponent: webComp
            onLoaded: item.loadHtml(root.htmlBody, "")
        }
    }

    Component {
        id: webComp
        WebEngineView {
            settings.javascriptEnabled: false
            settings.localContentCanAccessRemoteUrls: false
            settings.pluginsEnabled: false
            settings.autoLoadImages: root.loadRemoteImages
        }
    }

    Label {
        anchors.centerIn: parent
        visible: root.message === undefined
        text: qsTr("Select a message")
        opacity: 0.5
    }
}
