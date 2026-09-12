import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Simple manager for recipients collected after successful sends.
Dialog {
    id: root
    title: qsTr("Contacts")
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 80 : 520, 520)
    height: Math.min(parent ? parent.height - 80 : 500, 500)
    padding: Theme.lg
    property var backend
    property var rows: []
    signal statusMessage(string text)

    function reload() {
        try { root.rows = JSON.parse(root.backend.contacts_json("")) } catch (e) { root.rows = [] }
    }
    onOpened: reload()
    background: Rectangle { color: Theme.bg; radius: Theme.radiusLg; border.width: 1; border.color: Theme.border }
    header: Rectangle {
        implicitHeight: 52
        color: "transparent"
        Label { anchors.verticalCenter: parent.verticalCenter; anchors.left: parent.left; anchors.leftMargin: Theme.lg; text: root.title; color: Theme.text; font.pixelSize: Theme.fontMedium; font.bold: true }
        Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.border }
    }
    footer: RowLayout {
        Item { Layout.fillWidth: true }
        AppButton { Layout.rightMargin: Theme.lg; Layout.bottomMargin: Theme.md; text: qsTr("Close"); onClicked: root.close() }
    }
    contentItem: ColumnLayout {
        spacing: Theme.sm
        Label { Layout.fillWidth: true; wrapMode: Text.Wrap; color: Theme.textMuted; font.pixelSize: Theme.fontSmall; text: qsTr("Recipients are collected after a successful send while suggestions are enabled in Settings.") }
        Label { visible: root.rows.length === 0; Layout.fillWidth: true; Layout.topMargin: Theme.lg; horizontalAlignment: Text.AlignHCenter; color: Theme.textMuted; text: qsTr("No contacts yet") }
        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: root.rows
            clip: true
            spacing: 2
            delegate: Rectangle {
                width: ListView.view.width
                implicitHeight: 46
                color: contactHover.hovered ? Theme.bgAlt : "transparent"
                radius: Theme.radius
                HoverHandler { id: contactHover }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.sm
                    anchors.rightMargin: Theme.xs
                    Label { Layout.fillWidth: true; text: modelData.name ? modelData.name + " <" + modelData.address + ">" : modelData.address; color: Theme.text; elide: Text.ElideRight }
                    Label { text: modelData.times_seen; color: Theme.textMuted; font.pixelSize: Theme.fontTiny }
                    IconButton { text: "✕"; tooltip: qsTr("Remove contact"); onClicked: { var r = root.backend.delete_contact(modelData.address); if (r !== "") root.statusMessage(r); root.reload() } }
                }
            }
        }
    }
}
