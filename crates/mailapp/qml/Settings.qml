import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient

// Settings dialog, backed by the Rust SettingsBridge (SQLite store).
// Explicit sync on open/save (no bindings that user edits could break).
Dialog {
    id: root
    title: qsTr("Settings")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 440, 440)
    anchors.centerIn: parent
    standardButtons: Dialog.Ok | Dialog.Cancel

    signal statusMessage(string text)

    SettingsBridge {
        id: settings
    }

    onOpened: {
        settings.load()
        copyBox.checked = settings.sent_copy_enabled
        imagesBox.checked = settings.load_remote_images
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        CheckBox {
            id: copyBox
            Layout.fillWidth: true
            text: qsTr("Save a copy of sent mail in Sent")
            onToggled: settings.sent_copy_enabled = checked
        }
        CheckBox {
            id: imagesBox
            Layout.fillWidth: true
            text: qsTr("Load remote images in HTML mail (not recommended)")
            onToggled: settings.load_remote_images = checked
        }
        Label {
            Layout.fillWidth: true
            text: qsTr("Database: ~/.local/share/mailclient/mailclient.sqlite")
            opacity: 0.7
            elide: Text.ElideLeft
            font.pixelSize: 12
        }
    }

    onAccepted: {
        settings.save()
        root.statusMessage(qsTr("Settings saved"))
    }
}
