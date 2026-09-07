import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// First-run / add-account dialog. Submit goes to the Rust bridge, which
// persists the account to SQLite + keyring. Content scrolls on small screens.
Dialog {
    id: root
    title: qsTr("Add account")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 520, 520)
    height: Math.min(parent ? parent.height - 60 : 620, 620)
    anchors.centerIn: parent

    signal statusMessage(string text)
    signal accountSubmit(string payload)

    function submit() {
        root.accountSubmit(JSON.stringify({
            name: nameField.text,
            email: emailField.text,
            imap_host: imapField.text,
            imap_port: imapPortField.text,
            imap_sec: imapSecBox.currentText,
            imap_user: imapUserField.text,
            password: passField.text,
            smtp_host: smtpField.text,
            smtp_port: smtpPortField.text,
            smtp_sec: smtpSecBox.currentText,
            smtp_user: smtpUserField.text
        }))
    }

    footer: RowLayout {
        Button {
            text: qsTr("Cancel")
            Layout.alignment: Qt.AlignRight
            onClicked: root.reject()
        }
        Button {
            text: qsTr("Save")
            Layout.alignment: Qt.AlignRight
            highlighted: true
            onClicked: root.submit()
        }
    }

    ScrollView {
        id: scroll
        anchors.fill: parent
        clip: true

        GridLayout {
            width: scroll.availableWidth
            columns: 2
            columnSpacing: 12
            rowSpacing: 8

            Label { text: qsTr("Name") }
            TextField { id: nameField; Layout.fillWidth: true; placeholderText: qsTr("Work") }

            Label { text: qsTr("Email") }
            TextField { id: emailField; Layout.fillWidth: true; placeholderText: qsTr("you@example.com"); inputMethodHints: Qt.ImhEmailCharactersOnly }

            Label { text: qsTr("IMAP host"); font.bold: true }
            TextField { id: imapField; Layout.fillWidth: true; placeholderText: qsTr("imap.example.com") }

            Label { text: qsTr("IMAP port") }
            TextField { id: imapPortField; Layout.fillWidth: true; text: "993"; inputMethodHints: Qt.ImhDigitsOnly; validator: IntValidator { bottom: 1; top: 65535 } }

            Label { text: qsTr("IMAP encryption") }
            ComboBox { id: imapSecBox; Layout.fillWidth: true; model: [qsTr("TLS"), qsTr("STARTTLS")] }

            Label { text: qsTr("IMAP user") }
            TextField { id: imapUserField; Layout.fillWidth: true; placeholderText: qsTr("you@example.com") }

            Label { text: qsTr("Password") }
            TextField { id: passField; Layout.fillWidth: true; echoMode: TextInput.Password }

            Label { text: qsTr("SMTP host"); font.bold: true }
            TextField { id: smtpField; Layout.fillWidth: true; placeholderText: qsTr("smtp.example.com") }

            Label { text: qsTr("SMTP port") }
            TextField { id: smtpPortField; Layout.fillWidth: true; text: "465"; inputMethodHints: Qt.ImhDigitsOnly; validator: IntValidator { bottom: 1; top: 65535 } }

            Label { text: qsTr("SMTP encryption") }
            ComboBox { id: smtpSecBox; Layout.fillWidth: true; model: [qsTr("TLS"), qsTr("STARTTLS")] }

            Label { text: qsTr("SMTP user") }
            TextField { id: smtpUserField; Layout.fillWidth: true; placeholderText: qsTr("same as IMAP user") }
        }
    }
}
