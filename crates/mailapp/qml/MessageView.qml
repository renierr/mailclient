import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtWebEngine

// Reader pane. `message` roles come from Rust feed:
// {subject, from, date, body_text, body_html, is_html, has_remote_images, body}.
// - Plain mail renders as PlainText (never shows HTML source as code).
// - HTML was sanitized in Rust (scripts/handlers/styles/remote gated);
//   WebEngine runs with JS/plugins off, remote images per setting + one-shot.
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

    // Derived, decided in Rust — no QML `<`/`>` guessing.
    property bool isHtml: message !== undefined && message.is_html === true
    property string plainBody: message ? (message.body_text !== undefined ? message.body_text : (message.body || "")) : ""
    property string htmlBody: message ? (message.body_html !== undefined ? message.body_html : "") : ""
    property bool hasRemote: message !== undefined && message.has_remote_images === true
    property bool allowRemoteOnce: false

    onMessageChanged: {
        // One-shot remote consent is per-message.
        root.allowRemoteOnce = false
        if (root.isHtml && bodyLoader.item)
            bodyLoader.item.loadHtml(wrapDoc(root.htmlBody), "")
    }
    onHtmlBodyChanged: {
        if (root.isHtml && bodyLoader.item)
            bodyLoader.item.loadHtml(wrapDoc(root.htmlBody), "")
    }
    onLoadRemoteImagesChanged: {
        if (root.isHtml && bodyLoader.item)
            bodyLoader.item.loadHtml(wrapDoc(root.htmlBody), "")
    }
    onAllowRemoteOnceChanged: {
        if (root.isHtml && bodyLoader.item)
            bodyLoader.item.loadHtml(wrapDoc(root.htmlBody), "")
    }

    function effectiveAutoLoad() {
        return root.loadRemoteImages || root.allowRemoteOnce
    }

    // Trusted wrapper added AFTER Rust sanitizing (so layout CSS is ours).
    function wrapDoc(inner) {
        return "<!DOCTYPE html><html><head><meta charset=\"utf-8\">"
            + "<style>body{font-family:sans-serif;font-size:14px;line-height:1.5;max-width:72ch;margin:12px;word-wrap:break-word}"
            + "img{max-width:100%;height:auto}pre{white-space:pre-wrap}table{border-collapse:collapse}td,th{padding:4px 8px}</style>"
            + "</head><body>" + inner + "</body></html>"
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

        // Privacy banner: sanitizer saw remote images but autoload is off.
        Frame {
            Layout.fillWidth: true
            visible: root.isHtml && root.hasRemote && !root.effectiveAutoLoad()
            RowLayout {
                width: parent.width
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    text: qsTr("Remote images blocked (tracking protection).")
                    font.pixelSize: 12
                }
                Button {
                    text: qsTr("Show once")
                    onClicked: {
                        root.allowRemoteOnce = true
                        root.statusMessage(qsTr("Remote images allowed for this message only"))
                    }
                }
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: !root.isHtml
            TextEdit {
                width: root.width - 32
                text: root.plainBody
                textFormat: TextEdit.PlainText
                wrapMode: TextEdit.Wrap
                readOnly: true
                selectByMouse: true
                color: palette.text
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
            onLoaded: item.loadHtml(wrapDoc(root.htmlBody), "")
        }
    }

    Component {
        id: webComp
        WebEngineView {
            settings.javascriptEnabled: false
            settings.localContentCanAccessRemoteUrls: false
            settings.pluginsEnabled: false
            settings.autoLoadImages: root.effectiveAutoLoad()
        }
    }

    Label {
        anchors.centerIn: parent
        visible: root.message === undefined
        text: qsTr("Select a message")
        opacity: 0.5
    }
}
