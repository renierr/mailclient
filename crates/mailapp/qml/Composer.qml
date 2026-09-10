import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Compose dialog: WYSIWYG TextArea (RichText) + HTML-source toggle.
// Sends rich HTML source as `body`/`body_html`; Rust (`resolve_bodies` +
// `compose_send_format` setting) derives plain/multipart resiliently and
// sanitizes outgoing HTML. Cc is wired (was silently dropped before).
Dialog {
    id: root
    title: qsTr("Compose")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 720, 720)
    height: Math.min(parent ? parent.height - 80 : 600, 600)
    anchors.centerIn: parent

    signal statusMessage(string text)
    signal sendRequested(string payload)

    background: Rectangle {
        color: Theme.bg
        radius: Theme.radiusLg
        border.width: 1
        border.color: Theme.border
    }

    property string accountEmail: ""
    property string sendFormat: "multipart"
    property bool sourceMode: false

    // Flaw F5: Cancel used to throw the draft away silently. Everything the
    // user typed sets this, and closing then asks first.
    property bool dirty: false

    function markClean() { root.dirty = false }

    function requestClose() {
        if (root.dirty)
            discardConfirm.open()
        else
            root.close()
    }

    function plainToHtmlQuote(t) {
        var esc = t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
        return "<blockquote>" + esc.replace(/\n/g, "<br>") + "</blockquote>"
    }

    function openForReply(message) {
        if (fromField.text === "")
            fromField.text = root.accountEmail
        if (message !== undefined) {
            toField.text = message.from || ""
            subjectField.text = "Re: " + (message.subject || "")
            var q = message.body_text !== undefined ? message.body_text : (message.snippet || "")
            bodyArea.text = "<p></p><p>—</p>" + plainToHtmlQuote("On " + (message.date || "") + ", " + (message.from || "") + " wrote:\n" + q)
        }
        root.sourceMode = false
        root.markClean()
        open()
    }

    function openForForward(message) {
        if (fromField.text === "")
            fromField.text = root.accountEmail
        if (message !== undefined) {
            toField.text = ""
            subjectField.text = "Fwd: " + (message.subject || "")
            var q = message.body_text !== undefined ? message.body_text : (message.snippet || "")
            bodyArea.text = "<p></p><p>— Forwarded message —<br>From: "
                + (message.from || "") + "<br>Date: " + (message.date || "") + "<br>Subject: "
                + (message.subject || "") + "</p>" + plainToHtmlQuote(q)
        }
        root.sourceMode = false
        root.markClean()
        open()
    }

    // A fresh compose must not inherit the previous message's text.
    function openBlank() {
        fromField.text = root.accountEmail
        toField.text = ""
        ccField.text = ""
        subjectField.text = ""
        bodyArea.text = ""
        sourceArea.text = ""
        root.sourceMode = false
        root.markClean()
        open()
    }

    // Wrap the selection (or caret) in tags. Works on the RichText source.
    function wrapSelection(before, after) {
        if (root.sourceMode)
            return
        var s = bodyArea.selectionStart
        var e = bodyArea.selectionEnd
        if (s === e) {
            bodyArea.insert(s, before + after)
            bodyArea.cursorPosition = s + before.length
        } else {
            var sel = bodyArea.selectedText
            bodyArea.remove(s, e)
            bodyArea.insert(s, before + sel + after)
            bodyArea.cursorPosition = s + before.length + sel.length + after.length
        }
        bodyArea.forceActiveFocus()
    }

    function toggleSource() {
        if (!root.sourceMode) {
            sourceArea.text = bodyArea.text
            root.sourceMode = true
        } else {
            bodyArea.text = sourceArea.text
            root.sourceMode = false
        }
    }

    function clearFormatting() {
        // Strip to text paragraphs: RichText -> plain lines -> <p> blocks.
        var t = bodyArea.getText ? bodyArea.getText(0, bodyArea.length) : bodyArea.text.replace(/<[^>]*>/g, "")
        var esc = t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
        var parts = esc.split(/\n\n+/)
        var html = ""
        for (var i = 0; i < parts.length; i++)
            html += "<p>" + parts[i].replace(/\n/g, "<br>") + "</p>"
        bodyArea.text = html === "" ? "<p></p>" : html
        if (root.sourceMode)
            sourceArea.text = bodyArea.text
    }

    function collectPayload() {
        var html = root.sourceMode ? sourceArea.text : bodyArea.text
        return JSON.stringify({
            from: fromField.text,
            to: toField.text,
            cc: ccField.text,
            subject: subjectField.text,
            body: html,
            body_html: html
        })
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        // Flaw F4: these were placeholder-only, so the fields read as blank
        // boxes once filled in. Every field is labelled now.
        FormField {
            id: fromField
            Layout.fillWidth: true
            label: qsTr("From")
            placeholderText: qsTr("defaults to the account address")
            onTextChanged: root.dirty = true
        }
        FormField {
            id: toField
            Layout.fillWidth: true
            label: qsTr("To")
            required: true
            placeholderText: qsTr("name@example.com, second@example.com")
            onTextChanged: root.dirty = true
        }
        FormField {
            id: ccField
            Layout.fillWidth: true
            label: qsTr("Cc")
            placeholderText: qsTr("optional, comma separated")
            onTextChanged: root.dirty = true
        }
        FormField {
            id: subjectField
            Layout.fillWidth: true
            label: qsTr("Subject")
            onTextChanged: root.dirty = true
        }

        RowLayout {
            ToolButton {
                text: qsTr("B")
                font.bold: true
                Accessible.name: qsTr("Bold")
                enabled: !root.sourceMode
                onClicked: root.wrapSelection("<b>", "</b>")
            }
            ToolButton {
                text: qsTr("I")
                font.italic: true
                Accessible.name: qsTr("Italic")
                enabled: !root.sourceMode
                onClicked: root.wrapSelection("<i>", "</i>")
            }
            ToolButton {
                text: qsTr("U")
                font.underline: true
                Accessible.name: qsTr("Underline")
                enabled: !root.sourceMode
                onClicked: root.wrapSelection("<u>", "</u>")
            }
            ToolButton {
                text: qsTr("🔗")
                Accessible.name: qsTr("Insert link")
                enabled: !root.sourceMode
                onClicked: root.wrapSelection("<a href=\"https://\">", "</a>")
            }
            ToolButton {
                text: qsTr("•≡")
                Accessible.name: qsTr("Bullet list")
                enabled: !root.sourceMode
                onClicked: root.wrapSelection("<ul><li>", "</li></ul>")
            }
            ToolButton {
                text: qsTr("❝")
                Accessible.name: qsTr("Quote")
                enabled: !root.sourceMode
                onClicked: root.wrapSelection("<blockquote>", "</blockquote>")
            }
            ToolButton {
                text: qsTr("✕")
                Accessible.name: qsTr("Clear formatting")
                onClicked: root.clearFormatting()
            }
            ToolButton {
                text: root.sourceMode ? qsTr("&lt;/&gt; ✓") : qsTr("&lt;/&gt;")
                Accessible.name: qsTr("Toggle HTML source")
                highlighted: root.sourceMode
                onClicked: root.toggleSource()
            }
            Label {
                Layout.fillWidth: true
                horizontalAlignment: Text.AlignRight
                opacity: 0.6
                font.pixelSize: 11
                text: qsTr("Send as: %1").arg(root.sendFormat)
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: !root.sourceMode
            TextArea {
                id: bodyArea
                placeholderText: qsTr("Write your message… (toolbar edits real HTML)")
                wrapMode: TextArea.Wrap
                textFormat: TextArea.RichText
                selectByMouse: true
                color: Theme.text
                onTextChanged: root.dirty = true
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            visible: root.sourceMode
            TextArea {
                id: sourceArea
                placeholderText: qsTr("HTML source…")
                wrapMode: TextArea.Wrap
                textFormat: TextArea.PlainText
                font.family: "monospace"
                selectByMouse: true
                color: Theme.text
                onTextChanged: root.dirty = true
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.sm
            Label {
                text: root.dirty ? qsTr("Unsaved draft") : ""
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
            }
            Item { Layout.fillWidth: true }
            Button {
                text: qsTr("Discard")
                onClicked: root.requestClose()
            }
            Button {
                text: qsTr("Save draft")
                onClicked: {
                    root.statusMessage(qsTr("Drafts land with the send path in M2"))
                    root.markClean()
                    root.close()
                }
            }
            Button {
                text: qsTr("Send")
                highlighted: true
                enabled: toField.text.trim() !== ""
                onClicked: root.sendRequested(root.collectPayload())
            }
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
        standardButtons: Dialog.Cancel | Dialog.Discard

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        onDiscarded: {
            root.markClean()
            discardConfirm.close()
            root.close()
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
