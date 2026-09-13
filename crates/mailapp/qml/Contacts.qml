import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Manager for contacts, aliases, and suggestions.
Dialog {
    id: root
    title: qsTr("Manage Contacts")
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 80 : 600, 600)
    height: Math.min(parent ? parent.height - 80 : 540, 540)
    padding: Theme.lg
    property var backend
    property var rows: []
    property string editingAddress: ""
    property string searchQuery: ""
    signal statusMessage(string text)

    function reload() {
        try {
            root.rows = JSON.parse(root.backend.contacts_json(root.searchQuery))
        } catch (e) {
            root.rows = []
        }
    }

    function saveAlias(addr, newAlias) {
        var r = root.backend.update_contact_alias(addr, newAlias)
        if (r !== "") {
            root.statusMessage(r)
        } else {
            root.editingAddress = ""
            root.reload()
        }
    }

    onOpened: {
        root.searchQuery = ""
        root.editingAddress = ""
        root.reload()
    }

    background: Rectangle {
        color: Theme.bg
        radius: Theme.radiusLg
        border.width: 1
        border.color: Theme.border
    }

    header: Rectangle {
        implicitHeight: 52
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
        Item { Layout.fillWidth: true }
        AppButton {
            Layout.rightMargin: Theme.lg
            Layout.bottomMargin: Theme.md
            text: qsTr("Close")
            onClicked: root.close()
        }
    }

    contentItem: ColumnLayout {
        spacing: Theme.md

        Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            text: qsTr("Contacts are auto-collected from transferred real names in mail headers. You can customize the alias name for any contact.")
        }

        // Search bar
        RowLayout {
            Layout.fillWidth: true
            spacing: Theme.sm

            AppTextField {
                id: searchInput
                Layout.fillWidth: true
                placeholderText: qsTr("Search by alias, name, domain, address…")
                text: root.searchQuery
                onTextEdited: {
                    root.searchQuery = text
                    root.reload()
                }
            }

            IconButton {
                visible: root.searchQuery !== ""
                text: "✕"
                tooltip: qsTr("Clear search")
                onClicked: {
                    root.searchQuery = ""
                    searchInput.text = ""
                    root.reload()
                }
            }
        }

        Label {
            visible: root.rows.length === 0
            Layout.fillWidth: true
            Layout.topMargin: Theme.lg
            horizontalAlignment: Text.AlignHCenter
            color: Theme.textMuted
            text: root.searchQuery === "" ? qsTr("No contacts yet") : qsTr("No matching contacts found")
        }

        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: root.rows
            clip: true
            spacing: Theme.xs

            delegate: Rectangle {
                id: rowDelegate
                width: ListView.view.width
                implicitHeight: isEditing ? Math.round(76 * Theme.uiScale) : Math.round(56 * Theme.uiScale)
                color: isEditing ? Theme.bgRaised : (contactHover.hovered ? Theme.bgAlt : "transparent")
                radius: Theme.radius
                border.width: isEditing ? 1 : 0
                border.color: Theme.border

                readonly property bool isEditing: root.editingAddress === modelData.address

                HoverHandler { id: contactHover }

                // Display Mode
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.md
                    anchors.rightMargin: Theme.sm
                    visible: !rowDelegate.isEditing
                    spacing: Theme.sm

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2

                        RowLayout {
                            spacing: Theme.xs
                            Label {
                                text: modelData.alias ? modelData.alias : qsTr("(No alias)")
                                color: modelData.alias ? Theme.text : Theme.textMuted
                                font.bold: !!modelData.alias
                                font.pixelSize: Theme.fontMedium
                                elide: Text.ElideRight
                            }
                            Label {
                                text: "<" + modelData.address + ">"
                                color: Theme.textMuted
                                font.pixelSize: Theme.fontSmall
                                elide: Text.ElideRight
                            }
                        }

                        Label {
                            text: modelData.name && modelData.name !== modelData.alias ? qsTr("Transferred real name: ") + modelData.name : ""
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                            visible: text !== ""
                            elide: Text.ElideRight
                        }
                    }

                    Rectangle {
                        color: Theme.bgRaised
                        radius: Theme.radiusSm
                        implicitWidth: seenLabel.implicitWidth + Theme.sm * 2
                        implicitHeight: 20
                        border.width: 1
                        border.color: Theme.border
                        Label {
                            id: seenLabel
                            anchors.centerIn: parent
                            text: qsTr("seen: %1").arg(modelData.times_seen)
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                        }
                    }

                    IconButton {
                        text: "✎"
                        tooltip: qsTr("Edit alias")
                        onClicked: {
                            root.editingAddress = modelData.address
                        }
                    }

                    IconButton {
                        text: "✕"
                        tooltip: qsTr("Remove contact")
                        onClicked: {
                            var r = root.backend.delete_contact(modelData.address)
                            if (r !== "") root.statusMessage(r)
                            root.reload()
                        }
                    }
                }

                // Edit Mode
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.md
                    anchors.rightMargin: Theme.sm
                    visible: rowDelegate.isEditing
                    spacing: Theme.sm

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4

                        Label {
                            text: qsTr("Edit alias for %1").arg(modelData.address)
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                        }

                        AppTextField {
                            id: aliasEditField
                            Layout.fillWidth: true
                            text: modelData.alias || ""
                            placeholderText: modelData.name ? qsTr("Alias (default: %1)").arg(modelData.name) : qsTr("Alias name…")
                            Component.onCompleted: {
                                if (rowDelegate.isEditing) {
                                    forceActiveFocus()
                                    selectAll()
                                }
                            }
                            Keys.onReturnPressed: root.saveAlias(modelData.address, text.trim())
                            Keys.onEnterPressed: root.saveAlias(modelData.address, text.trim())
                            Keys.onEscapePressed: root.editingAddress = ""
                        }
                    }

                    IconButton {
                        text: "✓"
                        tooltip: qsTr("Save alias")
                        onClicked: root.saveAlias(modelData.address, aliasEditField.text.trim())
                    }

                    IconButton {
                        text: "✕"
                        tooltip: qsTr("Cancel")
                        onClicked: root.editingAddress = ""
                    }
                }
            }
        }
    }
}
