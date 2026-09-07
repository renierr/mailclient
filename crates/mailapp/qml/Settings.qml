import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Settings dialog, backed by the shared SettingsBridge passed in from Main.
// Explicit sync on open/save (no bindings that user edits could break).
Dialog {
    id: root
    title: qsTr("Settings")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 440, 440)
    anchors.centerIn: parent
    standardButtons: Dialog.Ok | Dialog.Cancel

    signal statusMessage(string text)

    // Shared bridge owned by Main (single source of truth).
    property var settingsBridge

    onOpened: {
        settingsBridge.load()
        copyBox.checked = settingsBridge.sent_copy_enabled
        imagesBox.checked = settingsBridge.load_remote_images
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        CheckBox {
            id: copyBox
            Layout.fillWidth: true
            text: qsTr("Save a copy of sent mail in Sent")
            onToggled: settingsBridge.sent_copy_enabled = checked
        }
        CheckBox {
            id: imagesBox
            Layout.fillWidth: true
            text: qsTr("Load remote images in HTML mail (not recommended)")
            onToggled: settingsBridge.load_remote_images = checked
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
        settingsBridge.save()
        root.statusMessage(qsTr("Settings saved"))
    }
}
