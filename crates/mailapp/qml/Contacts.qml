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
    property bool reviewing: false
    property var candidates: []
    property var selectedAddresses: []
    signal statusMessage(string text)

    function escapeHtml(t) {
        return (t || "").toString().replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g,
                                                                                                               "&quot;").replace(
                    /'/g, "&#39;");
    }

    function reload() {
        root.rows = FeedJson.parse(root.backend.contacts_json(root.searchQuery), []);
    }

    function reloadCandidates() {
        root.candidates = FeedJson.parse(root.backend.cleanup_candidates_json(), []);
    }

    function reasonText(reasons) {
        var parts = [];
        if ((reasons || []).indexOf("automated") >= 0)
            parts.push(qsTr("looks like an automated sender"));
        if ((reasons || []).indexOf("stale") >= 0)
            parts.push(qsTr("seen only once, long ago"));
        return parts.join("; ");
    }

    function enterReview() {
        root.reviewing = true;
        root.selectedAddresses = [];
        root.editingAddress = "";
        root.reloadCandidates();
    }

    function exitReview() {
        root.reviewing = false;
        root.selectedAddresses = [];
        root.reload();
    }

    function toggleSelected(addr) {
        var next = root.selectedAddresses.slice();
        var i = next.indexOf(addr);
        if (i >= 0)
            next.splice(i, 1);
        else
            next.push(addr);
        root.selectedAddresses = next;
    }

    function removeSelected() {
        var r = root.backend.delete_contacts(JSON.stringify(root.selectedAddresses));
        if (r !== "") {
            root.statusMessage(r);
            return;
        }
        root.selectedAddresses = [];
        root.reloadCandidates();
        root.reload();
    }

    function saveAlias(addr, newAlias) {
        var r = root.backend.update_contact_alias(addr, newAlias);
        if (r !== "") {
            root.statusMessage(r);
        } else {
            root.editingAddress = "";
            root.reload();
        }
    }

    onOpened: {
        root.searchQuery = "";
        root.editingAddress = "";
        root.reviewing = false;
        root.selectedAddresses = [];
        root.reload();
    }

    footer: RowLayout {
        spacing: Theme.sm
        AppButton {
            Layout.bottomMargin: Theme.md
            visible: !root.reviewing
            text: qsTr("Review suggestions…")
            Accessible.name: qsTr("Review contacts suggested for removal")
            onClicked: root.enterReview()
        }
        AppButton {
            Layout.bottomMargin: Theme.md
            visible: root.reviewing
            text: qsTr("Back")
            Accessible.name: qsTr("Back to all contacts")
            onClicked: root.exitReview()
        }
        Item {
            Layout.fillWidth: true
        }
        AppButton {
            Layout.bottomMargin: Theme.md
            visible: root.reviewing
            enabled: root.selectedAddresses.length > 0
            text: qsTr("Remove selected (%1)").arg(root.selectedAddresses.length)
            Accessible.name: qsTr("Remove selected contacts")
            onClicked: root.removeSelected()
        }
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
            text: root.reviewing ? qsTr(
                                       "These look like automated senders or addresses seen only once, long ago. Tick the ones to forget.") :
                                   qsTr("Contacts are auto-collected from transferred real names in mail headers. You can customize the alias name for any contact.")
        }

        // Search bar
        RowLayout {
            visible: !root.reviewing
            Layout.fillWidth: true
            spacing: Theme.sm

            AppTextField {
                id: searchInput
                Layout.fillWidth: true
                placeholderText: qsTr("Search by alias, name, domain, address…")
                text: root.searchQuery
                Accessible.name: qsTr("Search contacts")
                onTextEdited: {
                    root.searchQuery = text;
                    root.reload();
                }
                Keys.onDownPressed: {
                    if (contactList.count > 0) {
                        contactList.forceActiveFocus();
                        contactList.currentIndex = 0;
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
                    root.searchQuery = "";
                    searchInput.text = "";
                    root.reload();
                    searchInput.forceActiveFocus();
                }
            }
        }

        Label {
            visible: root.reviewing ? root.candidates.length === 0 : root.rows.length === 0
            Layout.fillWidth: true
            Layout.topMargin: Theme.lg
            horizontalAlignment: Text.AlignHCenter
            color: Theme.textMuted
            text: root.reviewing ? qsTr("No cleanup suggestions — your list looks tidy") : (root.searchQuery === "" ? qsTr(
                                                                                                                          "No contacts yet") :
                                                                                                                      qsTr("No matching contacts found"))
        }

        ListView {
            id: contactList
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: root.reviewing ? root.candidates : root.rows
            clip: true
            spacing: Theme.xs
            activeFocusOnTab: true
            keyNavigationEnabled: true
            highlightFollowsCurrentItem: true
            ScrollBar.vertical: ScrollBar {
                policy: ScrollBar.AsNeeded
            }

            Keys.onReturnPressed: {
                if (!root.reviewing && currentIndex >= 0 && currentIndex < count && root.editingAddress === "") {
                    root.editingAddress = model[currentIndex].address;
                }
            }
            Keys.onDeletePressed: {
                if (!root.reviewing && currentIndex >= 0 && currentIndex < count && root.editingAddress === "") {
                    var addr = model[currentIndex].address;
                    var r = root.backend.delete_contact(addr);
                    if (r !== "")
                        root.statusMessage(r);
                    root.reload();
                }
            }
            Keys.onSpacePressed: {
                if (root.reviewing && currentIndex >= 0 && currentIndex < count) {
                    root.toggleSelected(model[currentIndex].contact.address);
                }
            }

            delegate: Rectangle {
                id: rowDelegate
                width: ListView.view.width
                implicitHeight: {
                    var contentH = isEditing ? editLayout.implicitHeight : displayLayout.implicitHeight;
                    var baseH = Math.round((isEditing ? 72 : 54) * Theme.uiScale);
                    return Math.max(baseH, contentH + Theme.sm * 2);
                }
                color: isEditing ? Theme.bgRaised : (contactHover.hovered || ListView.isCurrentItem ? Theme.bgAlt :
                                                                                                      "transparent")
                radius: Theme.radius
                border.width: isEditing ? 1 : (ListView.isCurrentItem ? 1 : 0)
                border.color: isEditing ? Theme.accent : Theme.border

                // In review mode the model holds {contact, reasons}; otherwise the contact itself.
                readonly property var entry: root.reviewing ? modelData.contact : modelData
                readonly property string entryAddress: entry.address
                readonly property bool entrySelected: root.selectedAddresses.indexOf(entryAddress) >= 0
                readonly property bool isEditing: !root.reviewing && root.editingAddress === entryAddress

                HoverHandler {
                    id: contactHover
                }

                TapHandler {
                    onTapped: {
                        if (root.reviewing)
                            root.toggleSelected(entryAddress);
                    }
                    onDoubleTapped: {
                        if (!root.reviewing)
                            root.editingAddress = entryAddress;
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

                    CheckBox {
                        Layout.alignment: Qt.AlignVCenter
                        visible: root.reviewing
                        checked: rowDelegate.entrySelected
                        Accessible.name: qsTr("Select %1 for removal").arg(rowDelegate.entryAddress)
                        onToggled: root.toggleSelected(rowDelegate.entryAddress)
                    }

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
                                var aliasPart = entry.alias ? ("<span style='font-size: " + Theme.fontMedium
                                                               + "px; font-weight: bold; color: " + Theme.text + ";'>"
                                                               + root.escapeHtml(entry.alias) + "</span>") : (
                                                                  "<span style='font-size: " + Theme.fontMedium
                                                                  + "px; color: " + Theme.textMuted
                                                                  + "; font-style: italic;'>" + qsTr("(No alias)")
                                                                  + "</span>");
                                var addrPart = "<span style='font-size: " + Theme.fontSmall + "px; color: "
                                        + Theme.textMuted + ";'>&lt;" + root.escapeHtml(entryAddress) + "&gt;</span>";
                                return aliasPart + " " + addrPart;
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            text: entry.name && entry.name !== entry.alias ? qsTr("Transferred real name: ")
                                                                             + root.escapeHtml(entry.name) : ""
                            textFormat: Text.StyledText
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                            visible: text !== ""
                        }

                        Label {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            wrapMode: Text.WrapAtWordBoundaryOrAnywhere
                            text: root.reasonText(modelData.reasons)
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                            font.italic: true
                            visible: root.reviewing
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
                        Accessible.name: qsTr("Seen %1 times").arg(entry.times_seen)

                        Label {
                            id: seenLabel
                            anchors.centerIn: parent
                            text: qsTr("seen: %1").arg(entry.times_seen)
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                        }
                    }

                    IconButton {
                        Layout.alignment: Qt.AlignVCenter
                        visible: !root.reviewing
                        text: Icons.edit
                        iconFont: true
                        tooltip: qsTr("Edit alias")
                        Accessible.name: qsTr("Edit alias for %1").arg(entry.alias || entryAddress)
                        onClicked: {
                            root.editingAddress = entryAddress;
                        }
                    }

                    IconButton {
                        Layout.alignment: Qt.AlignVCenter
                        visible: !root.reviewing
                        text: Icons.close
                        iconFont: true
                        tooltip: qsTr("Remove contact")
                        Accessible.name: qsTr("Remove contact %1").arg(entry.alias || entryAddress)
                        onClicked: {
                            var r = root.backend.delete_contact(entryAddress);
                            if (r !== "")
                                root.statusMessage(r);
                            root.reload();
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
                            text: qsTr("Edit alias for %1").arg(entryAddress)
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                        }

                        AppTextField {
                            id: aliasEditField
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            text: entry.alias || ""
                            placeholderText: entry.name ? qsTr("Alias (default: %1)").arg(entry.name) : qsTr(
                                                              "Alias name…")
                            Accessible.name: qsTr("Alias for %1").arg(entryAddress)
                            Component.onCompleted: {
                                if (rowDelegate.isEditing) {
                                    forceActiveFocus();
                                    selectAll();
                                }
                            }
                            Keys.onReturnPressed: root.saveAlias(entryAddress, text.trim())
                            Keys.onEnterPressed: root.saveAlias(entryAddress, text.trim())
                            Keys.onEscapePressed: root.editingAddress = ""
                        }
                    }

                    IconButton {
                        Layout.alignment: Qt.AlignVCenter
                        text: Icons.done
                        iconFont: true
                        tooltip: qsTr("Save alias")
                        Accessible.name: qsTr("Save alias")
                        onClicked: root.saveAlias(entryAddress, aliasEditField.text.trim())
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
