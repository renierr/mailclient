import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// First-run / add-account dialog. M0/M1: validates locally and reports;
// M1b persists the account to SQLite + keyring and starts IMAP sync.
// Content scrolls so the form stays usable on small screens.
Dialog {
    id: root
    title: qsTr("Add account")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 520, 520)
    height: Math.min(parent ? parent.height - 60 : 620, 620)
    anchors.centerIn: parent
    standardButtons: Dialog.Ok | Dialog.Cancel

    signal statusMessage(string text)

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

    onAccepted: {
        if (emailField.text === "" || imapField.text === "" || passField.text === "") {
            root.statusMessage(qsTr("Fill email, IMAP host and password (mock check)"))
            return
        }
        if (smtpField.text === "") {
            root.statusMessage(qsTr("Fill the SMTP host (mock check)"))
            return
        }
        root.statusMessage(qsTr("Account '%1' will persist + sync in M1b").arg(emailField.text))
    }
}
