import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Add / edit account. Submit goes to the Rust bridge, which persists to SQLite
// + the OS keyring. Re-saving a known email address updates it in place.
Dialog {
    id: root
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 80 : 560, 560)
    height: Math.min(parent ? parent.height - 60 : 640, 640)
    padding: Theme.lg
    closePolicy: Popup.NoAutoClose

    // Set when editing an existing account; "" for a new one.
    property int editId: -1
    property bool editing: editId >= 0

    title: editing ? qsTr("Edit account") : qsTr("Add account")

    signal statusMessage(string text)
    signal accountSubmit(string payload)

    background: Rectangle {
        color: Theme.bg
        radius: Theme.radiusLg
        border.width: 1
        border.color: Theme.border
    }

    header: Rectangle {
        implicitHeight: 52
        color: "transparent"
        ColumnLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: Theme.lg
            anchors.rightMargin: Theme.lg
            spacing: 0
            Label {
                text: root.title
                color: Theme.text
                font.pixelSize: Theme.fontMedium
                font.bold: true
            }
            Label {
                text: root.editing ? qsTr("Leave the password blank to keep the stored one")
                                   : qsTr("Passwords are stored in the OS keyring, never in the database")
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
            }
        }
        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.border
        }
    }

    // Empty for a new account, prefilled from `Bridge.account_form` for an edit.
    function loadForm(json, id) {
        var f = {}
        try {
            f = JSON.parse(json || "{}")
        } catch (e) {
            f = {}
        }
        root.editId = id === undefined ? -1 : id
        nameField.text = f.name || ""
        emailField.text = f.email || ""
        fromNameField.text = f.from_name || ""
        imapField.text = f.imap_host || ""
        imapPortField.text = f.imap_port || "993"
        imapSecBox.currentIndex = (f.imap_sec || "tls").toLowerCase() === "starttls" ? 1 : 0
        imapUserField.text = f.imap_user || ""
        passField.text = ""
        smtpField.text = f.smtp_host || ""
        smtpPortField.text = f.smtp_port || "465"
        smtpSecBox.currentIndex = (f.smtp_sec || "tls").toLowerCase() === "starttls" ? 1 : 0
        smtpUserField.text = f.smtp_user || ""
        smtpPassField.text = ""
        root.clearErrors()
    }

    function openNew() {
        root.loadForm("{}", -1)
        root.open()
    }

    function openEdit(json, id) {
        root.loadForm(json, id)
        root.open()
    }

    function clearErrors() {
        emailField.invalid = false
        imapField.invalid = false
        smtpField.invalid = false
        passField.invalid = false
        errorLabel.text = ""
    }

    // Validate here so the dialog can point at the offending field; the
    // bridge re-checks anyway.
    function validate() {
        root.clearErrors()
        var problems = []
        if (emailField.text.trim() === "" || emailField.text.indexOf("@") < 0) {
            emailField.invalid = true
            problems.push(qsTr("a valid email address"))
        }
        if (imapField.text.trim() === "") {
            imapField.invalid = true
            problems.push(qsTr("the IMAP host"))
        }
        if (smtpField.text.trim() === "") {
            smtpField.invalid = true
            problems.push(qsTr("the SMTP host"))
        }
        if (!root.editing && passField.text === "") {
            passField.invalid = true
            problems.push(qsTr("a password"))
        }
        if (problems.length > 0) {
            errorLabel.text = qsTr("Please fill in %1.").arg(problems.join(", "))
            return false
        }
        return true
    }

    function submit() {
        if (!root.validate())
            return
        root.accountSubmit(JSON.stringify({
            name: nameField.text,
            email: emailField.text.trim(),
            from_name: fromNameField.text.trim(),
            imap_host: imapField.text.trim(),
            imap_port: imapPortField.text,
            imap_sec: imapSecBox.currentText.toLowerCase(),
            imap_user: imapUserField.text.trim() === "" ? emailField.text.trim() : imapUserField.text.trim(),
            password: passField.text,
            smtp_host: smtpField.text.trim(),
            smtp_port: smtpPortField.text,
            smtp_sec: smtpSecBox.currentText.toLowerCase(),
            smtp_user: smtpUserField.text.trim(),
            smtp_password: smtpPassField.text
        }))
    }

    // Fill the obvious hosts/user from the address once it is typed.
    function guessFromEmail() {
        var at = emailField.text.indexOf("@")
        if (at < 0)
            return
        var domain = emailField.text.substring(at + 1).trim()
        if (domain === "")
            return
        if (imapField.text === "")
            imapField.text = "imap." + domain
        if (smtpField.text === "")
            smtpField.text = "smtp." + domain
        if (imapUserField.text === "")
            imapUserField.text = emailField.text.trim()
    }

    footer: ColumnLayout {
        spacing: Theme.sm
        Label {
            id: errorLabel
            Layout.fillWidth: true
            Layout.leftMargin: Theme.lg
            Layout.rightMargin: Theme.lg
            visible: text !== ""
            color: Theme.danger
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.Wrap
        }
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Theme.lg
            Layout.rightMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            spacing: Theme.sm
            Item { Layout.fillWidth: true }
            AppButton {
                text: qsTr("Cancel")
                onClicked: root.reject()
            }
            AppButton {
                text: root.editing ? qsTr("Save changes") : qsTr("Add account")
                intent: "primary"
                onClicked: root.submit()
            }
        }
    }

    contentItem: ScrollView {
        id: scroll
        clip: true
        contentWidth: availableWidth

        ColumnLayout {
            width: scroll.availableWidth
            spacing: Theme.md

            // --- identity ---------------------------------------------------
            Label {
                text: qsTr("IDENTITY")
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
                font.bold: true
                font.letterSpacing: 1
            }
            FormField {
                id: emailField
                Layout.fillWidth: true
                label: qsTr("Email address")
                required: true
                placeholderText: "you@example.com"
                inputMethodHints: Qt.ImhEmailCharactersOnly
                onEditingFinished: root.guessFromEmail()
            }
            FormField {
                id: nameField
                Layout.fillWidth: true
                label: qsTr("Display name")
                placeholderText: qsTr("Work")
                hint: qsTr("Shown in the account list; defaults to the email address.")
            }
            FormField {
                id: fromNameField
                Layout.fillWidth: true
                label: qsTr("Sender name")
                placeholderText: qsTr("John Doe")
                hint: qsTr("Shown as the sender (From:) on outgoing mail; empty sends the address only.")
            }

            // --- incoming ---------------------------------------------------
            Label {
                text: qsTr("INCOMING MAIL (IMAP)")
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
                font.bold: true
                font.letterSpacing: 1
                Layout.topMargin: Theme.sm
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.sm
                FormField {
                    id: imapField
                    Layout.fillWidth: true
                    label: qsTr("Host")
                    required: true
                    placeholderText: "imap.example.com"
                }
                FormField {
                    id: imapPortField
                    Layout.preferredWidth: 90
                    label: qsTr("Port")
                    text: "993"
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator { bottom: 1; top: 65535 }
                }
                ColumnLayout {
                    spacing: Theme.xs
                    Label {
                        text: qsTr("Encryption")
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                    }
                    AppComboBox {
                        id: imapSecBox
                        Layout.preferredWidth: 120
                        model: ["TLS", "STARTTLS"]
                    }
                }
            }
            FormField {
                id: imapUserField
                Layout.fillWidth: true
                label: qsTr("Username")
                placeholderText: qsTr("defaults to the email address")
            }
            FormField {
                id: passField
                Layout.fillWidth: true
                label: qsTr("Password")
                required: !root.editing
                echoMode: TextInput.Password
                placeholderText: root.editing ? qsTr("unchanged") : ""
                hint: qsTr("Stored in the OS keyring (Credential Manager / Secret Service / Keychain).")
            }

            // --- outgoing ---------------------------------------------------
            Label {
                text: qsTr("OUTGOING MAIL (SMTP)")
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
                font.bold: true
                font.letterSpacing: 1
                Layout.topMargin: Theme.sm
            }
            RowLayout {
                Layout.fillWidth: true
                spacing: Theme.sm
                FormField {
                    id: smtpField
                    Layout.fillWidth: true
                    label: qsTr("Host")
                    required: true
                    placeholderText: "smtp.example.com"
                }
                FormField {
                    id: smtpPortField
                    Layout.preferredWidth: 90
                    label: qsTr("Port")
                    text: "465"
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator { bottom: 1; top: 65535 }
                }
                ColumnLayout {
                    spacing: Theme.xs
                    Label {
                        text: qsTr("Encryption")
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                    }
                    AppComboBox {
                        id: smtpSecBox
                        Layout.preferredWidth: 120
                        model: ["TLS", "STARTTLS"]
                    }
                }
            }
            FormField {
                id: smtpUserField
                Layout.fillWidth: true
                label: qsTr("Username")
                placeholderText: qsTr("same as IMAP user")
            }
            FormField {
                id: smtpPassField
                Layout.fillWidth: true
                label: qsTr("Password")
                echoMode: TextInput.Password
                placeholderText: qsTr("same as IMAP password")
            }
            Item { Layout.preferredHeight: Theme.sm }
        }
    }
}
