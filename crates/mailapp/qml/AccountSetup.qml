import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Add / edit account. Submit goes to the Rust bridge, which persists to SQLite
// + the OS keyring. An edit sends its account id, so changing the address
// renames that account; a new form with a known address updates it in place.
// Defaults, host guesses, port changes and the field check all come from
// `mailcore::store::account_form` through the bridge, shared with Flutter.
AppDialog {
    id: root
    preferredWidth: 560
    preferredHeight: 640
    minWidth: 420
    minHeight: 400
    padding: Theme.lg
    closePolicy: Popup.NoAutoClose

    required property var backend

    // Set when editing an existing account; -1 for a new one.
    property int editId: -1
    // Security choices from the core, labelled here.
    property var securityChoices: []
    property string imapSec: "tls"
    property string smtpSec: "tls"
    property var warnings: ({})
    property bool editing: editId >= 0

    title: editing ? qsTr("Edit account") : qsTr("Add account")
    subtitle: editing ? qsTr("Leave the password blank to keep the stored one") : qsTr(
                            "Passwords are stored in the OS keyring, never in the database")

    signal statusMessage(string text)
    signal accountSubmit(string payload)

    function securityLabel(value) {
        if (value === "starttls")
            return "STARTTLS";
        if (value === "none")
            return qsTr("None (unencrypted)");
        return "SSL/TLS";
    }

    function securityIndex(value) {
        var i = root.securityChoices.indexOf(value);
        return i < 0 ? 0 : i;
    }

    // Empty for a new account, prefilled from `Bridge.account_form` for an edit.
    function loadForm(json, id) {
        var d = FeedJson.parse(root.backend.account_form_defaults(), ({}));
        var f = FeedJson.parse(json, ({}));
        root.securityChoices = d.security_choices || ["tls", "starttls", "none"];
        root.editId = id === undefined ? -1 : id;
        nameField.text = f.name || "";
        emailField.text = f.email || "";
        fromNameField.text = f.from_name || "";
        imapField.text = f.imap_host || "";
        imapPortField.text = f.imap_port || d.imap_port || "";
        root.imapSec = f.imap_sec || d.imap_sec || "tls";
        imapUserField.text = f.imap_user || "";
        passField.text = "";
        smtpField.text = f.smtp_host || "";
        smtpPortField.text = f.smtp_port || d.smtp_port || "";
        root.smtpSec = f.smtp_sec || d.smtp_sec || "tls";
        smtpUserField.text = f.smtp_user || "";
        smtpPassField.text = "";
        root.clearErrors();
        root.refreshWarnings();
    }

    function openNew() {
        root.loadForm("{}", -1);
        root.open();
    }

    function openEdit(json, id) {
        root.loadForm(json, id);
        root.open();
    }

    function clearErrors() {
        emailField.invalid = false;
        imapField.invalid = false;
        imapPortField.invalid = false;
        smtpField.invalid = false;
        smtpPortField.invalid = false;
        passField.invalid = false;
        errorLabel.text = "";
    }

    function payload() {
        return JSON.stringify({
                                  id: root.editId,
                                  name: nameField.text,
                                  email: emailField.text.trim(),
                                  from_name: fromNameField.text.trim(),
                                  imap_host: imapField.text.trim(),
                                  imap_port: imapPortField.text,
                                  imap_sec: root.imapSec,
                                  imap_user: imapUserField.text.trim(),
                                  password: passField.text,
                                  smtp_host: smtpField.text.trim(),
                                  smtp_port: smtpPortField.text,
                                  smtp_sec: root.smtpSec,
                                  smtp_user: smtpUserField.text.trim(),
                                  smtp_password: smtpPassField.text
                              });
    }

    function check() {
        return FeedJson.parse(root.backend.account_form_check(root.payload(), root.editing), ({
                                                                                                  "errors": {},
                                                                                                  "warnings": {}
                                                                                              }));
    }

    function refreshWarnings() {
        root.warnings = root.check().warnings || {};
    }

    // The core's check points at the offending fields; save re-checks anyway.
    function validate() {
        root.clearErrors();
        var errors = root.check().errors || {};
        var fields = {
            "email": emailField,
            "imap_host": imapField,
            "imap_port": imapPortField,
            "smtp_host": smtpField,
            "smtp_port": smtpPortField,
            "password": passField
        };
        var problems = [];
        for (var key in errors) {
            if (fields[key] !== undefined)
                fields[key].invalid = true;
            problems.push(errors[key]);
        }
        errorLabel.text = problems.join(" · ");
        return problems.length === 0;
    }

    function submit() {
        if (!root.validate())
            return;
        root.accountSubmit(root.payload());
    }

    // Fill the obvious hosts/user from the address once it is typed. Never
    // overwrites a field the user filled.
    function guessFromEmail() {
        var g = FeedJson.parse(root.backend.account_guess(emailField.text), ({}));
        if (g.imap_host === undefined)
            return;
        if (imapField.text === "")
            imapField.text = g.imap_host;
        if (smtpField.text === "")
            smtpField.text = g.smtp_host;
        if (imapUserField.text === "")
            imapUserField.text = g.imap_user;
    }

    function setImapSec(value) {
        imapPortField.text = root.backend.account_port_for_security("imap", root.imapSec, value, imapPortField.text);
        root.imapSec = value;
        root.refreshWarnings();
    }

    function setSmtpSec(value) {
        smtpPortField.text = root.backend.account_port_for_security("smtp", root.smtpSec, value, smtpPortField.text);
        root.smtpSec = value;
        root.refreshWarnings();
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
            Layout.rightMargin: Theme.lg + 8
            Layout.bottomMargin: Theme.md
            spacing: Theme.sm
            Item {
                Layout.fillWidth: true
            }
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
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator {
                        bottom: 1
                        top: 65535
                    }
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
                        Layout.preferredWidth: 170
                        Layout.minimumWidth: 0
                        model: root.securityChoices.map(v => root.securityLabel(v))
                        currentIndex: root.securityIndex(root.imapSec)
                        onActivated: index => root.setImapSec(root.securityChoices[index])
                    }
                }
            }
            Label {
                Layout.fillWidth: true
                visible: text !== ""
                text: root.warnings.imap_sec || ""
                color: Theme.warning
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.Wrap
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
                password: true
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
                    inputMethodHints: Qt.ImhDigitsOnly
                    validator: IntValidator {
                        bottom: 1
                        top: 65535
                    }
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
                        Layout.preferredWidth: 170
                        Layout.minimumWidth: 0
                        model: root.securityChoices.map(v => root.securityLabel(v))
                        currentIndex: root.securityIndex(root.smtpSec)
                        onActivated: index => root.setSmtpSec(root.securityChoices[index])
                    }
                }
            }
            Label {
                Layout.fillWidth: true
                visible: text !== ""
                text: root.warnings.smtp_sec || ""
                color: Theme.warning
                font.pixelSize: Theme.fontSmall
                wrapMode: Text.Wrap
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
                password: true
                placeholderText: qsTr("same as IMAP password")
            }
            Item {
                Layout.preferredHeight: Theme.sm
            }
        }
    }
}
