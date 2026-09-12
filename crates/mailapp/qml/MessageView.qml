import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtWebEngine
import QtQuick.Dialogs
import QtCore

import Mailclient
import "components"

// Reader pane. `message` roles come from the Rust feed:
// {subject, from, date, body_text, body_html, is_html, has_remote_images}.
// - Plain mail renders as PlainText (never shows HTML source as code).
// - HTML was sanitized in Rust (scripts/handlers/styles stripped).
//   Inline `cid:`/`data:` images are part of the mail and always shown;
//   remote `http(s)` images are stripped unless the setting allows them or
//   the user taps "Show once" (re-sanitized on demand via `backend`).
// - WebEngine runs with JS/plugins off. `autoLoadImages` stays on so inline
//   images render; remote blocking is done by the sanitizer (which removed
//   the URLs) plus `localContentCanAccessRemoteUrls` (defense in depth).
Rectangle {
    id: root

    property var message
    property bool loadRemoteImages: false
    // Rust Bridge, for the on-demand "Show once" re-sanitize. Set by Main.
    property var backend
    property string remoteHtml: ""
    signal replyRequested()
    signal replyAllRequested()
    signal forwardRequested()
    signal starRequested()
    signal archiveRequested()
    signal moveRequested()
    signal deleteRequested()
    signal statusMessage(string text)

    color: Theme.bg

    // Derived, decided in Rust — no QML `<`/`>` guessing.
    readonly property bool isHtml: message !== undefined && message.is_html === true
    readonly property string plainBody: message ? (message.body_text !== undefined ? message.body_text : (message.body || "")) : ""
    readonly property string htmlBody: message ? (message.body_html !== undefined ? message.body_html : "") : ""
    readonly property bool hasRemote: message !== undefined && message.has_remote_images === true
    property bool allowRemoteOnce: false

    // Keyed on the UID, not on `message` itself: the feed is re-parsed after
    // every open, star, delete and sync, so `message` is a fresh object each
    // time even when it is the same mail. Reloading WebEngine on object
    // identity meant tearing the document down and back up on every click.
    readonly property int messageUid: message !== undefined && message !== null ? message.uid : -1

    onMessageUidChanged: {
        // One-shot remote consent is per-message.
        root.allowRemoteOnce = false
        root.remoteHtml = ""
        root.reloadHtml()
    }
    onHtmlBodyChanged: {
        // A fresh feed (sync, settings toggle) invalidates the one-shot copy.
        if (!root.allowRemoteOnce)
            root.remoteHtml = ""
        root.reloadHtml()
    }
    onRemoteHtmlChanged: root.reloadHtml()
    onLoadRemoteImagesChanged: root.reloadHtml()
    onAllowRemoteOnceChanged: root.reloadHtml()

    function reloadHtml() {
        if (root.isHtml && bodyLoader.item) {
            var body = root.remoteHtml !== "" ? root.remoteHtml : root.htmlBody
            bodyLoader.item.loadHtml(root.wrapDoc(body), "")
        }
    }

    function showRemoteOnce() {
        if (!root.message || root.message.uid === undefined)
            return
        // The feed stripped remote URLs (setting off), so re-sanitize the
        // stored raw body with remotes kept for this view only.
        var html = ""
        if (root.backend && root.backend.message_html)
            html = root.backend.message_html(root.message.uid, true)
        if (html !== "")
            root.remoteHtml = html
        root.allowRemoteOnce = true
        root.statusMessage(qsTr("Remote images allowed for this message only"))
    }

    function effectiveAutoLoad() {
        return root.loadRemoteImages || root.allowRemoteOnce
    }

    // Non-inline files from the Rust feed (bytes stay in SQLite until saved).
    readonly property var fileAttachments: {
        if (root.message === undefined || root.message === null
                || root.message.attachments === undefined)
            return []
        var out = []
        for (var i = 0; i < root.message.attachments.length; i++) {
            if (root.message.attachments[i].is_inline !== true)
                out.push(root.message.attachments[i])
        }
        return out
    }

    function formatSize(n) {
        if (n === undefined || n === null)
            return ""
        if (n < 1024)
            return qsTr("%1 B").arg(n)
        if (n < 1024 * 1024)
            return qsTr("%1 KB").arg((n / 1024).toFixed(1))
        return qsTr("%1 MB").arg((n / (1024 * 1024)).toFixed(1))
    }

    function displayName(a) {
        return a.filename || qsTr("attachment-%1.bin").arg(a.id)
    }

    function saveOne(a) {
        if (!root.backend || !root.backend.save_attachment)
            return
        saveOneDialog.attachmentId = a.id
        var base = StandardPaths.writableLocation(StandardPaths.DownloadLocation)
        saveOneDialog.selectedFile = "file://" + base + "/" + root.displayName(a)
        saveOneDialog.open()
    }

    function saveAll() {
        if (!root.backend || !root.backend.save_all_attachments)
            return
        saveAllDialog.open()
    }

    // Open in the system viewer — downloads first when not cached yet.
    function openOne(a) {
        if (!root.backend || !root.backend.open_attachment)
            return
        var url = root.backend.open_attachment(a.id)
        if (url.indexOf("file://") === 0)
            Qt.openUrlExternally(url)
        else
            root.statusMessage(url)
    }

    // Header details for the Headers dialog (Roundcube-style "Kopfzeilen"):
    // fetched on demand, never part of the feed rows.
    property var headersInfo: ({})

    function joinAddrs(v) {
        if (v === undefined || v === null)
            return ""
        if (typeof v === "string")
            return v
        if (v.length === undefined)
            return ""
        var out = []
        for (var i = 0; i < v.length; i++)
            out.push(v[i])
        return out.join(", ")
    }

    function openHeaders() {
        if (!root.backend || !root.backend.message_headers_json)
            return
        try {
            root.headersInfo = JSON.parse(root.backend.message_headers_json(root.messageUid))
        } catch (e) {
            root.headersInfo = ({})
        }
        headersDialog.open()
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
                    IconButton {
                        text: "⋯"
                        tooltip: qsTr("More actions")
                        onClicked: moreMenu.popup()
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
                AppButton {
                    text: qsTr("Show once")
                    onClicked: root.showRemoteOnce()
                }
            }
        }

        // --- attachments --------------------------------------------------
        // Names/sizes sync with the mail; bytes stay on the server until the
        // user explicitly opens or saves a file (offline-first). Opening
        // downloads into a temp copy for the system viewer; saving downloads
        // too when the bytes are not cached yet.
        // Inline images are part of the body and not listed here.
        Rectangle {
            Layout.fillWidth: true
            Layout.margins: Theme.md
            Layout.bottomMargin: 0
            implicitHeight: attachCol.implicitHeight + Theme.sm * 2
            visible: root.fileAttachments.length > 0
            radius: Theme.radius
            color: Theme.bgAlt
            border.width: 1
            border.color: Theme.border

            ColumnLayout {
                id: attachCol
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Theme.sm
                spacing: Theme.xs

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Theme.sm
                    Label {
                        text: "📎"
                    }
                    Label {
                        Layout.fillWidth: true
                        text: qsTr("%n attachment(s)", "", root.fileAttachments.length)
                        color: Theme.text
                        font.pixelSize: Theme.fontSmall
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    AppButton {
                        text: qsTr("Save all")
                        visible: root.fileAttachments.length > 1
                        onClicked: root.saveAll()
                    }
                }

                Repeater {
                    model: root.fileAttachments
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Theme.sm
                        id: fileRow
                        required property var modelData
                        Label {
                            Layout.fillWidth: true
                            text: root.displayName(fileRow.modelData)
                            color: Theme.text
                            font.pixelSize: Theme.fontSmall
                            elide: Text.ElideRight
                        }
                        Label {
                            text: root.formatSize(fileRow.modelData.size)
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                        }
                        AppButton {
                            text: qsTr("Open")
                            onClicked: root.openOne(fileRow.modelData)
                        }
                        AppButton {
                            text: qsTr("Save")
                            onClicked: root.saveOne(fileRow.modelData)
                        }
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
            // Inline cid:/data: images must render even when remote is
            // blocked, and the sanitizer already removed remote URLs — so
            // image loading stays on. Remote blocking for the allow-listed
            // case is enforced here: local content may only reach out when
            // the user consented (setting or Show-once).
            settings.autoLoadImages: true
            settings.localContentCanAccessRemoteUrls: root.effectiveAutoLoad()
            settings.pluginsEnabled: false
        }
    }

    // --- overflow menu ----------------------------------------------------
    // Less-common actions live here; shortcuts (R/F/A/M/…) still work.
    AppMenu {
        id: moreMenu

        MenuItem {
            text: qsTr("Reply all")
            onTriggered: root.replyAllRequested()
        }
        MenuItem {
            text: qsTr("Archive (A)")
            onTriggered: root.archiveRequested()
        }
        MenuItem {
            text: qsTr("Move to… (M)")
            onTriggered: root.moveRequested()
        }
        MenuSeparator {}
        MenuItem {
            text: qsTr("Show headers…")
            onTriggered: root.openHeaders()
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

    // --- save dialogs -----------------------------------------------------
    FileDialog {
        id: saveOneDialog
        title: qsTr("Save attachment")
        fileMode: FileDialog.SaveFile
        property int attachmentId: -1
        onAccepted: {
            if (root.backend && root.backend.save_attachment)
                root.statusMessage(
                    root.backend.save_attachment(attachmentId, selectedFile.toString()))
        }
    }

    FolderDialog {
        id: saveAllDialog
        title: qsTr("Save all attachments")
        onAccepted: {
            if (root.backend && root.backend.save_all_attachments)
                root.statusMessage(
                    root.backend.save_all_attachments(root.messageUid, selectedFolder.toString()))
        }
    }

    // --- headers dialog ---------------------------------------------------
    Dialog {
        id: headersDialog
        title: qsTr("Headers")
        modal: true
        anchors.centerIn: parent
        width: Math.min(parent ? parent.width - 120 : 520, 520)
        padding: Theme.lg

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        footer: RowLayout {
            spacing: Theme.sm
            Item { Layout.fillWidth: true }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                text: qsTr("Close")
                onClicked: headersDialog.close()
            }
        }

        GridLayout {
            width: parent.width
            columns: 2
            columnSpacing: Theme.md
            rowSpacing: Theme.xs

            Label { text: qsTr("From"); color: Theme.textMuted; font.pixelSize: Theme.fontSmall }
            Label {
                Layout.fillWidth: true
                text: root.headersInfo.from || ""
                color: Theme.text
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.WrapAnywhere
                textFormat: Text.PlainText
            }
            Label { text: qsTr("To"); color: Theme.textMuted; font.pixelSize: Theme.fontSmall }
            Label {
                Layout.fillWidth: true
                text: root.joinAddrs(root.headersInfo.to)
                color: Theme.text
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.WrapAnywhere
                textFormat: Text.PlainText
            }
            Label {
                text: qsTr("Cc")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                visible: root.joinAddrs(root.headersInfo.cc) !== ""
            }
            Label {
                Layout.fillWidth: true
                text: root.joinAddrs(root.headersInfo.cc)
                color: Theme.text
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.WrapAnywhere
                textFormat: Text.PlainText
                visible: text !== ""
            }
            Label { text: qsTr("Date"); color: Theme.textMuted; font.pixelSize: Theme.fontSmall }
            Label {
                Layout.fillWidth: true
                text: root.headersInfo.date || ""
                color: Theme.text
                font.pixelSize: Theme.fontSmall
                textFormat: Text.PlainText
            }
            Label { text: qsTr("Subject"); color: Theme.textMuted; font.pixelSize: Theme.fontSmall }
            Label {
                Layout.fillWidth: true
                text: root.headersInfo.subject || ""
                color: Theme.text
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.Wrap
                textFormat: Text.PlainText
            }
            Label {
                text: qsTr("Message-ID")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                visible: (root.headersInfo.message_id || "") !== ""
            }
            Label {
                Layout.fillWidth: true
                text: root.headersInfo.message_id || ""
                color: Theme.text
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.WrapAnywhere
                textFormat: Text.PlainText
                visible: text !== ""
            }
            Label {
                text: qsTr("Reply-To")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                visible: (root.headersInfo.reply_to || "") !== ""
            }
            Label {
                Layout.fillWidth: true
                text: root.headersInfo.reply_to || ""
                color: Theme.text
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.WrapAnywhere
                textFormat: Text.PlainText
                visible: text !== ""
            }
        }
    }
}
