import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs

import Mailclient
import "components"

// Compose dialog: real WYSIWYG body (components/EditorFrame.qml) with an
// HTML-source toggle.
//
// The toolbar drives `document.execCommand`, so text changes visibly as you
// press B/I/U and the buttons light up to show what the caret sits inside
// (flaw F3). It also emits semantic <b>/<i>/<u> tags, which the outgoing
// sanitizer keeps — Qt's rich-text TextArea emitted inline styles that were
// stripped on send, so formatting silently never arrived.
//
// Rust (`resolve_bodies` + the `compose_send_format`/`compose_include_plain`
// settings) derives the plain/multipart shape and sanitizes the HTML:
// Auto sends text/plain unless the body carries real formatting.
Dialog {
    id: root
    title: qsTr("Compose")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 760, 760)
    height: Math.min(parent ? parent.height - 60 : 640, 640)
    anchors.centerIn: parent
    padding: Theme.lg
    closePolicy: Popup.NoAutoClose

    signal statusMessage(string text)
    signal sendRequested(string payload)

    property string accountEmail: ""
    property string sendFormat: "auto"
    property bool sourceMode: false

    // Cc/Bcc rows stay collapsed until toggled (or non-empty).
    property bool showCc: false
    property bool showBcc: false

    // Outgoing files picked via FileDialog: [{path, name}]. Paths (plain or
    // `file://` URLs) travel in the send payload; Rust reads the bytes at
    // send time, so no binary crosses the QML bridge.
    property var attachments: []

    // Flaw F5: Cancel used to throw the draft away silently. Everything the
    // user types sets this, and closing then asks first.
    property bool dirty: false

    // The domain is fixed to the account: only the local part is editable,
    // since sending as another domain fails SPF/DMARC anyway.
    readonly property string accountDomain: {
        var at = root.accountEmail.indexOf("@")
        return at < 0 ? "" : root.accountEmail.substring(at)
    }
    readonly property string accountLocalPart: {
        var at = root.accountEmail.indexOf("@")
        return at < 0 ? root.accountEmail : root.accountEmail.substring(0, at)
    }
    readonly property string effectiveFrom:
        fromLocal.text.trim() === "" ? root.accountEmail
                                     : fromLocal.text.trim() + root.accountDomain

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
        Label {
            anchors.verticalCenter: parent.verticalCenter
            anchors.right: parent.right
            anchors.rightMargin: Theme.lg
            text: root.sendFormat === "auto" ? qsTr("Send as: Auto") : qsTr("Send as: %1").arg(root.sendFormat)
            color: Theme.textMuted
            font.pixelSize: Theme.fontTiny
        }
        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.border
        }
    }

    function markClean() { root.dirty = false }

    function requestClose() {
        if (root.dirty)
            discardConfirm.open()
        else
            root.close()
    }

    function escapeHtml(t) {
        return t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    }

    // Plain-text quote as `>` citations: renders in every client and keeps
    // Auto sends as text/plain unless the user adds real formatting.
    function plainToHtmlQuote(t) {
        var lines = root.escapeHtml(t).split("\n")
        for (var i = 0; i < lines.length; i++)
            lines[i] = "&gt; " + lines[i]
        return "<p>" + lines.join("<br>") + "</p>"
    }

    // Quote in the format the original arrived in: HTML mail gets a styled
    // <blockquote> of its HTML body, plain mail gets `>` citations of its
    // text. The toolbar Quote button still inserts a styled blockquote on
    // explicit request. Bodies come pre-sanitized from the Rust feed and are
    // sanitized again on send.
    function quoteBody(message, headerText) {
        var wasHtml = message.is_html === true
        var html = message.body_html !== undefined ? message.body_html : ""
        if (wasHtml && html !== "")
            return "<p></p><p>" + root.escapeHtml(headerText) + "</p><blockquote>" + html + "</blockquote>"
        var q = message.body_text !== undefined ? message.body_text : (message.snippet || "")
        return "<p></p>" + root.plainToHtmlQuote(headerText + "\n" + q)
    }

    function resetHeaders() {
        fromLocal.text = root.accountLocalPart
        toField.text = ""
        ccField.text = ""
        bccField.text = ""
        subjectField.text = ""
        root.showCc = false
        root.showBcc = false
        root.attachments = []
    }

    function setBody(html) {
        bodyEditor.setHtml(html)
        sourceArea.text = html
    }

    function openBlank() {
        root.sourceMode = false
        root.resetHeaders()
        root.setBody("")
        root.markClean()
        open()
    }

    function openForReply(message) {
        root.sourceMode = false
        root.resetHeaders()
        if (message !== undefined) {
            toField.text = message.from || ""
            subjectField.text = "Re: " + (message.subject || "")
            root.setBody(root.quoteBody(message,
                "On " + (message.date || "") + ", " + (message.from || "") + " wrote:"))
        }
        root.markClean()
        open()
    }

    function openForForward(message) {
        root.sourceMode = false
        root.resetHeaders()
        if (message !== undefined) {
            subjectField.text = "Fwd: " + (message.subject || "")
            var wasHtml = message.is_html === true
            var html = message.body_html !== undefined ? message.body_html : ""
            if (wasHtml && html !== "") {
                root.setBody("<p></p><p>— Forwarded message —<br>From: "
                    + root.escapeHtml(message.from || "") + "<br>Date: "
                    + root.escapeHtml(message.date || "") + "<br>Subject: "
                    + root.escapeHtml(message.subject || "") + "</p><blockquote>" + html + "</blockquote>")
            } else {
                var q = message.body_text !== undefined ? message.body_text : (message.snippet || "")
                root.setBody("<p></p>" + root.plainToHtmlQuote("— Forwarded message —\nFrom: "
                    + (message.from || "") + "\nDate: " + (message.date || "") + "\nSubject: "
                    + (message.subject || "") + "\n\n" + q))
            }
        }
        root.markClean()
        open()
    }

    // Source mode shows exactly what will be sent, and edits round-trip.
    function toggleSource() {
        if (!root.sourceMode) {
            bodyEditor.fetchHtml(function (html) {
                sourceArea.text = html
                root.sourceMode = true
            })
        } else {
            bodyEditor.setHtml(sourceArea.text)
            root.sourceMode = false
        }
    }

    function baseName(url) {
        var s = url.toString()
        var i = Math.max(s.lastIndexOf("/"), s.lastIndexOf("\\"))
        var name = i < 0 ? s : s.substring(i + 1)
        try { name = decodeURIComponent(name) } catch (e) {}
        return name === "" ? s : name
    }

    function addAttachments(urls) {
        var next = root.attachments.slice()
        for (var i = 0; i < urls.length; i++) {
            var u = urls[i].toString()
            var known = false
            for (var j = 0; j < next.length; j++) {
                if (next[j].path === u) {
                    known = true
                    break
                }
            }
            if (!known)
                next.push({ path: u, name: root.baseName(u) })
        }
        root.attachments = next
        root.dirty = true
    }

    function removeAttachment(index) {
        var next = root.attachments.slice()
        next.splice(index, 1)
        root.attachments = next
        root.dirty = true
    }

    function payloadFor(html) {
        var paths = []
        for (var i = 0; i < root.attachments.length; i++)
            paths.push(root.attachments[i].path)
        return JSON.stringify({
            from: root.effectiveFrom,
            to: toField.text,
            cc: ccField.text,
            bcc: bccField.text,
            subject: subjectField.text,
            body: html,
            body_html: html,
            attachments: paths
        })
    }

    // Reading the document back is asynchronous, so Send finishes inside the
    // callback rather than returning a payload.
    function requestSend() {
        if (toField.text.trim() === "") {
            root.statusMessage(qsTr("Add at least one recipient"))
            return
        }
        if (root.sourceMode) {
            root.sendRequested(root.payloadFor(sourceArea.text))
        } else {
            bodyEditor.fetchHtml(function (html) {
                root.sendRequested(root.payloadFor(html))
            })
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Theme.sm

        // --- headers ------------------------------------------------------
        // One row each for From / To / Subject; Cc and Bcc hide behind
        // toggles beside the To field until needed.
        GridLayout {
            Layout.fillWidth: true
            columns: 3
            columnSpacing: Theme.sm
            rowSpacing: Theme.xs

            Label {
                text: qsTr("From")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                Layout.preferredWidth: 52
            }
            // Local part editable, domain locked to the account.
            Rectangle {
                Layout.fillWidth: true
                Layout.columnSpan: 2
                implicitHeight: 32
                radius: Theme.radius
                color: Theme.bg
                border.width: 1
                border.color: fromLocal.activeFocus ? Theme.accent : Theme.border

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.sm
                    anchors.rightMargin: Theme.sm
                    spacing: 0

                    AppTextField {
                        id: fromLocal
                        Layout.fillWidth: true
                        text: root.accountLocalPart
                        color: Theme.text
                        font.pixelSize: Theme.fontBase
                        placeholderTextColor: Theme.textMuted
                        selectByMouse: true
                        leftPadding: 0
                        rightPadding: 0
                        background: null
                        onTextChanged: root.dirty = true
                    }
                    Label {
                        text: root.accountDomain
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontBase
                        ToolTip.visible: domainHover.hovered
                        ToolTip.text: qsTr("Fixed to this account's domain")
                        HoverHandler { id: domainHover }
                    }
                }
            }

            Label {
                text: qsTr("To")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
            }
            AppTextField {
                id: toField
                Layout.fillWidth: true
                placeholderText: qsTr("name@example.com, second@example.com")
                onTextChanged: root.dirty = true
            }
            Row {
                spacing: 2
                IconButton {
                    text: "Cc"
                    fontSize: Theme.fontSmall
                    implicitWidth: 36
                    implicitHeight: 32
                    active: root.showCc || ccField.text !== ""
                    tooltip: qsTr("Show Cc field")
                    onClicked: root.showCc = !root.showCc
                }
                IconButton {
                    text: qsTr("Bcc")
                    fontSize: Theme.fontSmall
                    implicitWidth: 40
                    implicitHeight: 32
                    active: root.showBcc || bccField.text !== ""
                    tooltip: qsTr("Show Bcc field")
                    onClicked: root.showBcc = !root.showBcc
                }
            }

            Label {
                text: qsTr("Cc")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                visible: root.showCc || ccField.text !== ""
            }
            AppTextField {
                id: ccField
                Layout.fillWidth: true
                Layout.columnSpan: 2
                visible: root.showCc || ccField.text !== ""
                placeholderText: qsTr("optional, comma separated")
                onTextChanged: root.dirty = true
            }

            Label {
                text: qsTr("Bcc")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                visible: root.showBcc || bccField.text !== ""
            }
            AppTextField {
                id: bccField
                Layout.fillWidth: true
                Layout.columnSpan: 2
                visible: root.showBcc || bccField.text !== ""
                placeholderText: qsTr("optional, hidden recipients")
                onTextChanged: root.dirty = true
            }

            Label {
                text: qsTr("Subject")
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
            }
            AppTextField {
                id: subjectField
                Layout.fillWidth: true
                Layout.columnSpan: 2
                onTextChanged: root.dirty = true
            }
        }

        // --- attachments ----------------------------------------------------
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: attachRow.implicitHeight + Theme.sm * 2
            visible: root.attachments.length > 0
            radius: Theme.radius
            color: Theme.bgAlt
            border.width: 1
            border.color: Theme.border

            RowLayout {
                id: attachRow
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: Theme.sm
                spacing: Theme.xs

                Label {
                    text: "📎"
                }
                Label {
                    text: qsTr("%n file(s)", "", root.attachments.length)
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                }
                // Chips wrap via Flow (names can be long).
                Flow {
                    Layout.fillWidth: true
                    spacing: Theme.xs
                    Repeater {
                        model: root.attachments
                        Rectangle {
                            id: chipBox
                            height: 26
                            width: chipRow.implicitWidth + Theme.sm * 2
                            radius: 13
                            color: Theme.bgRaised
                            border.width: 1
                            border.color: Theme.border
                            required property var modelData
                            required property int index
                            Row {
                                id: chipRow
                                anchors.centerIn: parent
                                spacing: 4
                                Label {
                                    anchors.verticalCenter: parent.verticalCenter
                                    text: chipBox.modelData.name
                                    color: Theme.text
                                    font.pixelSize: Theme.fontSmall
                                    elide: Text.ElideMiddle
                                    width: Math.min(implicitWidth, 180)
                                }
                                IconButton {
                                    anchors.verticalCenter: parent.verticalCenter
                                    width: 20
                                    height: 20
                                    fontSize: Theme.fontSmall
                                    text: "✕"
                                    tooltip: qsTr("Remove")
                                    onClicked: root.removeAttachment(chipBox.index)
                                }
                            }
                        }
                    }
                }
                AppButton {
                    text: qsTr("Add")
                    onClicked: attachDialog.open()
                }
            }
        }

        // --- formatting toolbar -------------------------------------------
        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 38
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
                    active: bodyEditor.boldActive
                    font.bold: true
                    onClicked: bodyEditor.exec("bold")
                }
                IconButton {
                    text: "I"
                    tooltip: qsTr("Italic (Ctrl+I)")
                    enabled: !root.sourceMode
                    active: bodyEditor.italicActive
                    font.italic: true
                    onClicked: bodyEditor.exec("italic")
                }
                IconButton {
                    text: "U"
                    tooltip: qsTr("Underline (Ctrl+U)")
                    enabled: !root.sourceMode
                    active: bodyEditor.underlineActive
                    font.underline: true
                    onClicked: bodyEditor.exec("underline")
                }

                Rectangle {
                    implicitWidth: 1
                    implicitHeight: 20
                    color: Theme.border
                }

                IconButton {
                    text: "•≡"
                    tooltip: qsTr("Bullet list")
                    enabled: !root.sourceMode
                    active: bodyEditor.listActive
                    onClicked: bodyEditor.exec("insertUnorderedList")
                }
                IconButton {
                    text: "❝"
                    tooltip: qsTr("Quote")
                    enabled: !root.sourceMode
                    active: bodyEditor.quoteActive
                    onClicked: bodyEditor.exec("formatBlock", bodyEditor.quoteActive ? "p" : "blockquote")
                }
                IconButton {
                    text: "🔗"
                    tooltip: qsTr("Insert link")
                    enabled: !root.sourceMode
                    onClicked: linkDialog.open()
                }
                IconButton {
                    text: "✕"
                    tooltip: qsTr("Clear formatting")
                    enabled: !root.sourceMode
                    onClicked: bodyEditor.exec("removeFormat")
                }
                IconButton {
                    text: "📎"
                    tooltip: qsTr("Attach files")
                    onClicked: attachDialog.open()
                }

                Item { Layout.fillWidth: true }

                IconButton {
                    text: "</>"
                    fontSize: Theme.fontSmall
                    implicitWidth: 40
                    tooltip: qsTr("Toggle HTML source")
                    active: root.sourceMode
                    onClicked: root.toggleSource()
                }
            }
        }

        // --- body ---------------------------------------------------------
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            radius: Theme.radius
            color: Theme.bg
            border.width: 1
            border.color: Theme.border
            clip: true

            EditorFrame {
                id: bodyEditor
                anchors.fill: parent
                anchors.margins: 1
                visible: !root.sourceMode
                onContentChanged: root.dirty = true
            }

            ScrollView {
                anchors.fill: parent
                anchors.margins: 1
                visible: root.sourceMode
                clip: true

                TextArea {
                    id: sourceArea
                    placeholderText: qsTr("HTML source…")
                    wrapMode: TextArea.Wrap
                    textFormat: TextArea.PlainText
                    font.family: "monospace"
                    font.pixelSize: Theme.fontSmall
                    color: Theme.text
                    placeholderTextColor: Theme.textMuted
                    selectByMouse: true
                    background: null
                    onTextChanged: root.dirty = true
                }
            }
        }

        // --- actions ------------------------------------------------------
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.sm
            Label {
                text: root.dirty ? qsTr("Unsaved draft") : ""
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
            }
            Item { Layout.fillWidth: true }
            AppButton {
                text: qsTr("Discard")
                onClicked: root.requestClose()
            }
            AppButton {
                text: qsTr("Save draft")
                onClicked: {
                    root.statusMessage(qsTr("Drafts land with the send path in M2"))
                    root.markClean()
                    root.close()
                }
            }
            AppButton {
                text: qsTr("Send")
                intent: "primary"
                enabled: toField.text.trim() !== ""
                onClicked: root.requestSend()
            }
        }
    }

    // --- file picker ------------------------------------------------------
    FileDialog {
        id: attachDialog
        title: qsTr("Attach files")
        fileMode: FileDialog.OpenFiles
        onAccepted: root.addAttachments(selectedFiles)
    }

    // --- link insertion ---------------------------------------------------
    Dialog {
        id: linkDialog
        title: qsTr("Insert link")
        modal: true
        anchors.centerIn: parent
        width: 420
        padding: Theme.lg

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        onOpened: urlField.text = "https://"

        footer: RowLayout {
            spacing: Theme.sm
            Item { Layout.fillWidth: true }
            AppButton {
                text: qsTr("Cancel")
                onClicked: linkDialog.close()
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                Layout.topMargin: Theme.sm
                text: qsTr("Insert")
                intent: "primary"
                onClicked: {
                    bodyEditor.exec("createLink", urlField.text.trim())
                    linkDialog.close()
                }
            }
        }

        FormField {
            id: urlField
            width: parent.width
            label: qsTr("Address")
            hint: qsTr("Select text first to turn it into a link.")
        }
    }

    // Flaw F5: never lose typed content without asking.
    Dialog {
        id: discardConfirm
        title: qsTr("Discard draft?")
        modal: true
        anchors.centerIn: parent
        width: 380
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
                text: qsTr("Keep editing")
                onClicked: discardConfirm.close()
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                Layout.topMargin: Theme.sm
                text: qsTr("Discard")
                intent: "danger"
                onClicked: {
                    root.markClean()
                    discardConfirm.close()
                    root.close()
                }
            }
        }

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            color: Theme.text
            font.pixelSize: Theme.fontBase
            text: qsTr("This message has not been sent. Discard it?")
        }
    }
}
