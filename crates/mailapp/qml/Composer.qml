import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

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
    standardButtons: Dialog.Cancel

    signal statusMessage(string text)
    signal sendRequested(string payload)

    property string accountEmail: ""
    property string sendFormat: "multipart"
    property bool sourceMode: false

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

        TextField {
            id: fromField
            Layout.fillWidth: true
            placeholderText: qsTr("From (defaults to account email)")
        }
        TextField {
            id: toField
            Layout.fillWidth: true
            placeholderText: qsTr("To (comma separated)")
        }
        TextField {
            id: ccField
            Layout.fillWidth: true
            placeholderText: qsTr("Cc (optional, comma separated)")
        }
        TextField {
            id: subjectField
            Layout.fillWidth: true
            placeholderText: qsTr("Subject")
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
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignRight
            Button {
                text: qsTr("Save draft")
                onClicked: {
                    root.statusMessage(qsTr("Drafts land with the send path in M2"))
                    root.close()
                }
            }
            Button {
                text: qsTr("Send")
                highlighted: true
                onClicked: root.sendRequested(root.collectPayload())
            }
        }
    }
}
