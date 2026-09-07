import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Compose dialog. M0: rich-text TextArea + mock Send/Draft.
// M2 wires Send to the Rust send_queue + lettre SMTP transport.
Dialog {
    id: root
    title: qsTr("Compose")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 720, 720)
    height: Math.min(parent ? parent.height - 80 : 560, 560)
    anchors.centerIn: parent
    standardButtons: Dialog.Cancel

    signal statusMessage(string text)
    signal sendRequested(string payload)

    property string accountEmail: ""

    function openForReply(message) {
        if (fromField.text === "")
            fromField.text = root.accountEmail
        if (message !== undefined) {
            toField.text = message.from || ""
            subjectField.text = "Re: " + (message.subject || "")
            bodyArea.text = "\n\n—\nOn " + (message.date || "") + ", " + (message.from || "") + " wrote:\n" + (message.snippet || "")
        }
        open()
    }

    function openForForward(message) {
        if (fromField.text === "")
            fromField.text = root.accountEmail
        if (message !== undefined) {
            toField.text = ""
            subjectField.text = "Fwd: " + (message.subject || "")
            bodyArea.text = "\n\n— Forwarded message —\nFrom: " + (message.from || "") + "\nDate: " + (message.date || "") + "\nSubject: " + (message.subject || "") + "\n\n" + (message.snippet || "")
        }
        open()
    }

    // Wrap the selection (or caret) in rich-text tags.
    function wrapSelection(before, after) {
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
            placeholderText: qsTr("To")
        }
        TextField {
            Layout.fillWidth: true
            placeholderText: qsTr("Cc (optional)")
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
                onClicked: root.wrapSelection("<b>", "</b>")
            }
            ToolButton {
                text: qsTr("I")
                font.italic: true
                Accessible.name: qsTr("Italic")
                onClicked: root.wrapSelection("<i>", "</i>")
            }
            ToolButton {
                text: qsTr("U")
                font.underline: true
                Accessible.name: qsTr("Underline")
                onClicked: root.wrapSelection("<u>", "</u>")
            }
            ToolButton {
                text: qsTr("🔗")
                Accessible.name: qsTr("Insert link")
                onClicked: root.wrapSelection("<a href=\"https://\">", "</a>")
            }
            ToolButton {
                text: qsTr("📎")
                Accessible.name: qsTr("Attach file")
                onClicked: root.statusMessage(qsTr("Attachments land with the send path in M2"))
            }
        }

        ScrollView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            TextArea {
                id: bodyArea
                placeholderText: qsTr("Write your message…")
                wrapMode: TextArea.Wrap
                textFormat: TextArea.RichText
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
                onClicked: {
                    root.sendRequested(JSON.stringify({
                        from: fromField.text,
                        to: toField.text,
                        subject: subjectField.text,
                        body: bodyArea.text
                    }))
                }
            }
        }
    }
}
