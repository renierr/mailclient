import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// First-run / add-account dialog. M0 validates locally and reports the
// values; M1 persists the account to SQLite + keyring and starts IMAP sync.
Dialog {
    id: root
    title: qsTr("Add account")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 480, 480)
    anchors.centerIn: parent
    standardButtons: Dialog.Ok | Dialog.Cancel

    signal statusMessage(string text)

    GridLayout {
        anchors.fill: parent
        columns: 2
        columnSpacing: 12
        rowSpacing: 8

        Label { text: qsTr("Name") }
        TextField { id: nameField; Layout.fillWidth: true; placeholderText: qsTr("Work") }
        Label { text: qsTr("Email") }
        TextField { id: emailField; Layout.fillWidth: true; placeholderText: qsTr("you@example.com"); inputMethodHints: Qt.ImhEmailCharactersOnly }
        Label { text: qsTr("IMAP host") }
        TextField { id: imapField; Layout.fillWidth: true; placeholderText: qsTr("imap.example.com") }
        Label { text: qsTr("IMAP user") }
        TextField { Layout.fillWidth: true; placeholderText: qsTr("you@example.com") }
        Label { text: qsTr("Password") }
        TextField { id: passField; Layout.fillWidth: true; echoMode: TextInput.Password }
        Label { text: qsTr("SMTP host") }
        TextField { Layout.fillWidth: true; placeholderText: qsTr("smtp.example.com") }
    }

    onAccepted: {
        if (emailField.text === "" || imapField.text === "" || passField.text === "") {
            root.statusMessage(qsTr("Fill email, IMAP host and password (mock check)"))
            return
        }
        root.statusMessage(qsTr("Account '%1' will persist + sync in M1").arg(emailField.text))
    }
}
