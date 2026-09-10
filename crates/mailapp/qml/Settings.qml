import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Settings dialog, backed by the shared SettingsBridge passed in from Main.
// Explicit sync on open/save (no bindings that user edits could break).
Dialog {
    id: root
    title: qsTr("Settings")
    modal: true
    width: Math.min(parent ? parent.width - 80 : 480, 480)
    anchors.centerIn: parent
    padding: Theme.lg

    signal statusMessage(string text)

    // Shared bridge owned by Main (single source of truth).
    property var settingsBridge
    // Passed in from Main: the DB path lives on Bridge, not SettingsBridge.
    property string dbPath: ""

    background: Rectangle {
        color: Theme.bg
        radius: Theme.radiusLg
        border.width: 1
        border.color: Theme.border
    }

    header: Rectangle {
        implicitHeight: 48
        color: "transparent"
        Label {
            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: Theme.lg
            text: root.title
            color: Theme.text
            font.pixelSize: Theme.fontMedium
            font.bold: true
        }
        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.border
        }
    }

    footer: RowLayout {
        spacing: Theme.sm
        Item { Layout.fillWidth: true }
        AppButton {
            text: qsTr("Cancel")
            onClicked: root.reject()
        }
        AppButton {
            Layout.rightMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            Layout.topMargin: Theme.sm
            text: qsTr("Save")
            intent: "primary"
            onClicked: root.accept()
        }
    }

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
        spacing: Theme.md

        AppCheckBox {
            id: copyBox
            Layout.fillWidth: true
            text: qsTr("Save a copy of sent mail in Sent")
            onToggled: root.settingsBridge.sent_copy_enabled = checked
        }
        AppCheckBox {
            id: imagesBox
            Layout.fillWidth: true
            text: qsTr("Load remote images in HTML mail (not recommended)")
            onToggled: root.settingsBridge.load_remote_images = checked
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            color: Theme.border
        }

        Label {
            text: qsTr("SEND MAIL AS")
            color: Theme.textMuted
            font.pixelSize: Theme.fontTiny
            font.bold: true
            font.letterSpacing: 1
        }
        AppComboBox {
            id: formatBox
            Layout.fillWidth: true
            model: [qsTr("Plain text (safest)"), qsTr("Multipart plain + HTML (recommended)"), qsTr("HTML only")]
            onActivated: index => {
                var v = ["plain", "multipart", "html"][index]
                root.settingsBridge.compose_send_format = v
            }
        }
        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            text: qsTr("Multipart sends plain + HTML so every client reads it; plain strips formatting; HTML-only may look broken in old clients.")
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            color: Theme.border
        }

        Label {
            Layout.fillWidth: true
            text: qsTr("Database: %1").arg(root.dbPath)
            color: Theme.textMuted
            elide: Text.ElideLeft
            font.pixelSize: Theme.fontSmall
        }
    }

    onAccepted: {
        settingsBridge.save()
        root.statusMessage(qsTr("Settings saved"))
    }
}
