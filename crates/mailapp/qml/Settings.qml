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

    function formatIndex(v) {
        if (v === "plain")
            return 0
        if (v === "html")
            return 2
        return 1
    }

    onOpened: {
        settingsBridge.load()
        copyBox.checked = settingsBridge.sent_copy_enabled
        imagesBox.checked = settingsBridge.load_remote_images
        formatBox.currentIndex = formatIndex(settingsBridge.compose_send_format)
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
            text: qsTr("Send mail as")
        }
        ComboBox {
            id: formatBox
            Layout.fillWidth: true
            model: [qsTr("Plain text (safest)"), qsTr("Multipart plain + HTML (recommended)"), qsTr("HTML only")]
            onActivated: index => {
                var v = ["plain", "multipart", "html"][index]
                settingsBridge.compose_send_format = v
            }
        }
        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            opacity: 0.7
            font.pixelSize: 12
            text: qsTr("Multipart sends plain + HTML so every client reads it; plain strips formatting; HTML-only may look broken in old clients.")
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
