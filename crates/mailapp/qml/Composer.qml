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

    function openForReply(message) {
        if (message !== undefined) {
            toField.text = message.from || ""
            subjectField.text = "Re: " + (message.subject || "")
            bodyArea.text = "\n\n—\nOn " + (message.date || "") + ", " + (message.from || "") + " wrote:\n" + (message.snippet || "")
        }
        open()
    }

    function openForForward(message) {
        if (message !== undefined) {
            toField.text = ""
            subjectField.text = "Fwd: " + (message.subject || "")
            bodyArea.text = "\n\n— Forwarded message —\nFrom: " + (message.from || "") + "\nDate: " + (message.date || "") + "\nSubject: " + (message.subject || "") + "\n\n" + (message.snippet || "")
        }
        open()
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

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
                onClicked: bodyArea.insert(bodyArea.cursorPosition, "<b></b>")
            }
            ToolButton {
                text: qsTr("I")
                font.italic: true
                Accessible.name: qsTr("Italic")
                onClicked: bodyArea.insert(bodyArea.cursorPosition, "<i></i>")
            }
            ToolButton {
                text: qsTr("🔗")
                Accessible.name: qsTr("Insert link")
                onClicked: bodyArea.insert(bodyArea.cursorPosition, "<a href=\"https://\">link</a>")
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
                        to: toField.text,
                        subject: subjectField.text,
                        body: bodyArea.text
                    }))
                }
            }
        }
    }
}
