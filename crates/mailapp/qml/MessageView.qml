import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtWebEngine

import Mailclient
import "components"

// Reader pane. `message` roles come from the Rust feed:
// {subject, from, date, body_text, body_html, is_html, has_remote_images}.
// - Plain mail renders as PlainText (never shows HTML source as code).
// - HTML was sanitized in Rust (scripts/handlers/styles/remote gated);
//   WebEngine runs with JS/plugins off, remote images per setting + one-shot.
Rectangle {
    id: root

    property var message
    property bool loadRemoteImages: false
    signal replyRequested()
    signal replyAllRequested()
    signal forwardRequested()
    signal starRequested()
    signal deleteRequested()
    signal statusMessage(string text)

    color: Theme.bg

    // Derived, decided in Rust — no QML `<`/`>` guessing.
    readonly property bool isHtml: message !== undefined && message.is_html === true
    readonly property string plainBody: message ? (message.body_text !== undefined ? message.body_text : (message.body || "")) : ""
    readonly property string htmlBody: message ? (message.body_html !== undefined ? message.body_html : "") : ""
    readonly property bool hasRemote: message !== undefined && message.has_remote_images === true
    property bool allowRemoteOnce: false

    onMessageChanged: {
        // One-shot remote consent is per-message.
        root.allowRemoteOnce = false
        root.reloadHtml()
    }
    onHtmlBodyChanged: root.reloadHtml()
    onLoadRemoteImagesChanged: root.reloadHtml()
    onAllowRemoteOnceChanged: root.reloadHtml()

    function reloadHtml() {
        if (root.isHtml && bodyLoader.item)
            bodyLoader.item.loadHtml(root.wrapDoc(root.htmlBody), "")
    }

    function effectiveAutoLoad() {
        return root.loadRemoteImages || root.allowRemoteOnce
    }

    // Trusted wrapper added AFTER Rust sanitizing (so layout CSS is ours).
    // Colours come from the theme so HTML mail matches the app in dark mode.
    function wrapDoc(inner) {
        return "<!DOCTYPE html><html><head><meta charset=\"utf-8\">"
            + "<style>body{font-family:sans-serif;font-size:14px;line-height:1.55;"
            + "max-width:78ch;margin:16px;word-wrap:break-word;"
            + "color:" + Theme.text + ";background:" + Theme.bg + "}"
            + "a{color:" + Theme.accent + "}"
            + "img{max-width:100%;height:auto}pre{white-space:pre-wrap}"
            + "blockquote{margin:8px 0;padding-left:12px;border-left:3px solid "
            + Theme.border + ";color:" + Theme.textMuted + "}"
            + "table{border-collapse:collapse}td,th{padding:4px 8px}</style>"
            + "</head><body>" + inner + "</body></html>"
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        visible: root.message !== undefined

        // --- header -------------------------------------------------------
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: headerCol.implicitHeight + Theme.lg * 2
            color: Theme.bg

            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: 1
                color: Theme.border
            }

            ColumnLayout {
                id: headerCol
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Theme.lg
                spacing: Theme.md

                Label {
                    Layout.fillWidth: true
                    text: root.message ? root.message.subject : ""
                    color: Theme.text
                    font.pixelSize: Theme.fontTitle
                    font.bold: true
                    wrapMode: Text.Wrap
                    maximumLineCount: 3
                    elide: Text.ElideRight
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.md

                    Avatar {
                        implicitWidth: 36
                        implicitHeight: 36
                        seed: root.message ? (root.message.from || "?") : "?"
                        initials: root.message ? (root.message.from || "?").replace(/^[^a-zA-Z0-9]*/, "").substring(0, 1).toUpperCase() : "?"
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Label {
                            text: root.message ? root.message.from : ""
                            color: Theme.text
                            font.pixelSize: Theme.fontBase
                            font.bold: true
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        Label {
                            text: root.message ? root.message.date : ""
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontSmall
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                    }

                    IconButton {
                        text: "↩"
                        tooltip: qsTr("Reply (R)")
                        onClicked: root.replyRequested()
                    }
                    IconButton {
                        text: "↩↩"
                        tooltip: qsTr("Reply all")
                        onClicked: root.replyAllRequested()
                    }
                    IconButton {
                        text: "→"
                        tooltip: qsTr("Forward (F)")
                        onClicked: root.forwardRequested()
                    }
                    IconButton {
                        text: root.message && root.message.starred ? "★" : "☆"
                        contentColor: root.message && root.message.starred ? Theme.star : Theme.text
                        tooltip: qsTr("Star (S)")
                        onClicked: root.starRequested()
                    }
                    IconButton {
                        text: "🗑"
                        tooltip: qsTr("Delete (Del)")
                        contentColor: Theme.danger
                        onClicked: root.deleteRequested()
                    }
                }
            }
        }

        // Privacy banner: sanitizer saw remote images but autoload is off.
        Rectangle {
            Layout.fillWidth: true
            Layout.margins: Theme.md
            implicitHeight: 40
            visible: root.isHtml && root.hasRemote && !root.effectiveAutoLoad()
            radius: Theme.radius
            color: Theme.bgAlt
            border.width: 1
            border.color: Theme.border

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Theme.md
                anchors.rightMargin: Theme.sm
                spacing: Theme.sm
                Label {
                    text: "🛡"
                }
                Label {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Theme.text
                    text: qsTr("Remote images blocked (tracking protection).")
                    font.pixelSize: Theme.fontSmall
                }
                Button {
                    text: qsTr("Show once")
                    flat: true
                    onClicked: {
                        root.allowRemoteOnce = true
                        root.statusMessage(qsTr("Remote images allowed for this message only"))
                    }
                }
            }
        }

        // --- body: plain ---------------------------------------------------
        // Flat Flickable + Text rather than ScrollView + TextEdit: the old
        // TextEdit had no height inside the ScrollView and rendered nothing
        // (flaw F2). Text still supports mouse selection via TextEdit-like
        // selection on the parent Flickable being unnecessary.
        Flickable {
            id: plainFlick
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: !root.isHtml
            clip: true
            contentWidth: width
            contentHeight: plainText.implicitHeight + Theme.lg * 2
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

            TextEdit {
                id: plainText
                x: Theme.lg
                y: Theme.lg
                width: plainFlick.width - Theme.lg * 2
                text: root.plainBody
                textFormat: TextEdit.PlainText
                wrapMode: TextEdit.Wrap
                readOnly: true
                selectByMouse: true
                color: Theme.text
                font.pixelSize: Theme.fontBase + 1
            }
        }

        // --- body: sanitized HTML -----------------------------------------
        Loader {
            id: bodyLoader
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: root.isHtml
            active: root.message !== undefined && root.isHtml
            sourceComponent: webComp
            onLoaded: root.reloadHtml()
        }
    }

    Component {
        id: webComp
        WebEngineView {
            backgroundColor: Theme.bg
            settings.javascriptEnabled: false
            settings.localContentCanAccessRemoteUrls: false
            settings.pluginsEnabled: false
            settings.autoLoadImages: root.effectiveAutoLoad()
        }
    }

    // --- empty state ------------------------------------------------------
    Column {
        anchors.centerIn: parent
        spacing: Theme.sm
        visible: root.message === undefined
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            text: "✉"
            font.pixelSize: 40
            color: Theme.textMuted
            opacity: 0.6
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            text: qsTr("Select a message to read it")
            color: Theme.textMuted
            font.pixelSize: Theme.fontBase
        }
    }
}
