import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Manager for contacts, aliases, and suggestions.
// Uses AppDialog for generic dragging, resizing, and host clamping.
AppDialog {
    id: root
    title: qsTr("Manage Contacts")
    preferredWidth: 640
    preferredHeight: 560
    minWidth: 420
    minHeight: 340
    padding: Theme.lg

    property var backend
    property var rows: []
    property string editingAddress: ""
    property string searchQuery: ""
    signal statusMessage(string text)

    function escapeHtml(t) {
        return (t || "").toString()
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#39;")
    }

    function reload() {
        root.rows = FeedJson.parse(root.backend.contacts_json(root.searchQuery), [])
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

    footer: RowLayout {
        spacing: Theme.sm
        Item { Layout.fillWidth: true }
        AppButton {
            Layout.rightMargin: Theme.lg + 8
            Layout.bottomMargin: Theme.md
            text: qsTr("Close")
            Accessible.name: qsTr("Close contacts manager")
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
                Accessible.name: qsTr("Search contacts")
                onTextEdited: {
                    root.searchQuery = text
                    root.reload()
                }
                Keys.onDownPressed: {
                    if (contactList.count > 0) {
                        contactList.forceActiveFocus()
                        contactList.currentIndex = 0
                    }
                }
            }

            IconButton {
                visible: root.searchQuery !== ""
                text: Icons.close
                iconFont: true
                tooltip: qsTr("Clear search")
                Accessible.name: qsTr("Clear search")
                onClicked: {
                    root.searchQuery = ""
                    searchInput.text = ""
                    root.reload()
                    searchInput.forceActiveFocus()
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
            id: contactList
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: root.rows
            clip: true
            spacing: Theme.xs
            activeFocusOnTab: true
            keyNavigationEnabled: true
            highlightFollowsCurrentItem: true

            Keys.onReturnPressed: {
                if (currentIndex >= 0 && currentIndex < count && root.editingAddress === "") {
                    root.editingAddress = model[currentIndex].address
                }
            }
            Keys.onDeletePressed: {
                if (currentIndex >= 0 && currentIndex < count && root.editingAddress === "") {
                    var addr = model[currentIndex].address
                    var r = root.backend.delete_contact(addr)
                    if (r !== "") root.statusMessage(r)
                    root.reload()
                }
            }

            delegate: Rectangle {
                id: rowDelegate
                width: ListView.view.width
                implicitHeight: {
                    var contentH = isEditing ? editLayout.implicitHeight : displayLayout.implicitHeight
                    var baseH = Math.round((isEditing ? 72 : 54) * Theme.uiScale)
                    return Math.max(baseH, contentH + Theme.sm * 2)
                }
                color: isEditing ? Theme.bgRaised
                                 : (contactHover.hovered || ListView.isCurrentItem ? Theme.bgAlt : "transparent")
                radius: Theme.radius
                border.width: isEditing ? 1 : (ListView.isCurrentItem ? 1 : 0)
                border.color: isEditing ? Theme.accent : Theme.border

                readonly property bool isEditing: root.editingAddress === modelData.address

                HoverHandler { id: contactHover }

                TapHandler {
                    onDoubleTapped: {
                        root.editingAddress = modelData.address
                    }
                }

                // Display Mode
                RowLayout {
                    id: displayLayout
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: Theme.md
                    anchors.rightMargin: Theme.sm
                    visible: !rowDelegate.isEditing
                    spacing: Theme.sm

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 2

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            textFormat: Text.StyledText
                            text: {
                                var aliasPart = modelData.alias
                                    ? ("<span style='font-size: " + Theme.fontMedium + "px; font-weight: bold; color: " + Theme.text + ";'>"
                                       + root.escapeHtml(modelData.alias) + "</span>")
                                    : ("<span style='font-size: " + Theme.fontMedium + "px; color: " + Theme.textMuted + "; font-style: italic;'>"
                                       + qsTr("(No alias)") + "</span>")
                                var addrPart = "<span style='font-size: " + Theme.fontSmall + "px; color: " + Theme.textMuted + ";'>&lt;"
                                    + root.escapeHtml(modelData.address) + "&gt;</span>"
                                return aliasPart + " " + addrPart
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            text: modelData.name && modelData.name !== modelData.alias ? qsTr("Transferred real name: ") + root.escapeHtml(modelData.name) : ""
                            textFormat: Text.StyledText
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                            visible: text !== ""
                        }
                    }

                    Rectangle {
                        Layout.alignment: Qt.AlignVCenter
                        color: Theme.bgRaised
                        radius: Theme.radius
                        implicitWidth: seenLabel.implicitWidth + Theme.sm * 2
                        implicitHeight: Theme.pillHeight
                        border.width: 1
                        border.color: Theme.border
                        Accessible.name: qsTr("Seen %1 times").arg(modelData.times_seen)

                        Label {
                            id: seenLabel
                            anchors.centerIn: parent
                            text: qsTr("seen: %1").arg(modelData.times_seen)
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                        }
                    }

                    IconButton {
                        Layout.alignment: Qt.AlignVCenter
                        text: Icons.edit
                        iconFont: true
                        tooltip: qsTr("Edit alias")
                        Accessible.name: qsTr("Edit alias for %1").arg(modelData.alias || modelData.address)
                        onClicked: {
                            root.editingAddress = modelData.address
                        }
                    }

                    IconButton {
                        Layout.alignment: Qt.AlignVCenter
                        text: Icons.close
                        iconFont: true
                        tooltip: qsTr("Remove contact")
                        Accessible.name: qsTr("Remove contact %1").arg(modelData.alias || modelData.address)
                        onClicked: {
                            var r = root.backend.delete_contact(modelData.address)
                            if (r !== "") root.statusMessage(r)
                            root.reload()
                        }
                    }
                }

                // Edit Mode
                RowLayout {
                    id: editLayout
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.leftMargin: Theme.md
                    anchors.rightMargin: Theme.sm
                    visible: rowDelegate.isEditing
                    spacing: Theme.sm

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 4

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            text: qsTr("Edit alias for %1").arg(modelData.address)
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                        }

                        AppTextField {
                            id: aliasEditField
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            text: modelData.alias || ""
                            placeholderText: modelData.name ? qsTr("Alias (default: %1)").arg(modelData.name) : qsTr("Alias name…")
                            Accessible.name: qsTr("Alias for %1").arg(modelData.address)
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
                        Layout.alignment: Qt.AlignVCenter
                        text: Icons.done
                        iconFont: true
                        tooltip: qsTr("Save alias")
                        Accessible.name: qsTr("Save alias")
                        onClicked: root.saveAlias(modelData.address, aliasEditField.text.trim())
                    }

                    IconButton {
                        Layout.alignment: Qt.AlignVCenter
                        text: Icons.close
                        iconFont: true
                        tooltip: qsTr("Cancel")
                        Accessible.name: qsTr("Cancel alias edit")
                        onClicked: root.editingAddress = ""
                    }
                }
            }
        }
    }
}
