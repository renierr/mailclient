import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Folder/account sidebar. Expects `folders` ListModel with
// {id, name, role, unread, subscribed, count, depth, leaf, always_visible}.
// Only subscribed (visible) folders are listed — the rest live in the
// Folders manager. Parents collapse (default closed); well-known folders
// (`always_visible`, e.g. an Archive filed below INBOX) always show, and a
// collapsed parent aggregates its hidden children's counts so no unread
// pill disappears with them.
Rectangle {
    id: root

    property var folders
    property var accounts
    property string currentFolder: ""
    property string currentEmail: ""
    property int currentAccountId: -1

    signal folderSelected(string path)
    signal accountSelected(int id)

    // Visible subset of `folders` (subscribed !== false). Kept as its own
    // model so hiding a folder never destroys the full feed the manager
    // dialog reads — same in-place update discipline as ModelSync.
    ListModel {
        id: shown
    }

    // Expanded parents, by folder id. In-memory: every launch starts
    // collapsed (default closed).
    property var expandedById: ({})

    function parentPathOf(f) {
        // Feed `leaf` is the last path segment; what precedes it (minus the
        // single-char IMAP delimiter) is the parent. Depth 0 has none, and a
        // parent outside the visible set counts as none — the child then
        // reads as a root, the way the old flat list showed it.
        if (f.depth === undefined || f.depth <= 0)
            return null;
        var leaf = f.leaf !== undefined ? f.leaf : "";
        var cut = f.name.length - leaf.length - 1;
        return cut > 0 ? f.name.substring(0, cut) : null;
    }

    function refreshShown() {
        var all = [];
        var byPath = {};
        if (root.folders) {
            for (var i = 0; i < root.folders.count; i++) {
                var f = root.folders.get(i);
                if (f.subscribed === false)
                    continue;
                var row = {
                    id: f.id,
                    name: f.name,
                    role: f.role,
                    unread: f.unread,
                    count: f.count !== undefined ? f.count : 0,
                    depth: f.depth !== undefined ? f.depth : 0,
                    leaf: f.leaf !== undefined && f.leaf !== "" ? f.leaf : f.name,
                    alwaysVisible: f.always_visible !== false
                };
                all.push(row);
                byPath[row.name] = row;
            }
        }
        var parentOf = function (row) {
            var p = parentPathOf(row);
            return p !== null && byPath[p] !== undefined ? byPath[p] : null;
        };
        var isShown = function (row) {
            if (row.depth <= 0 || row.alwaysVisible)
                return true;
            var p = parentOf(row);
            if (p === null)
                return true;
            return root.expandedById[p.id] === true && isShown(p);
        };
        // A row is collapsible only when its toggle hides something: a
        // direct child that folds away (custom role). INBOX, whose children
        // all stay visible, gets no chevron and stays inbox-only in counts.
        var canCollapse = function (row) {
            for (var i = 0; i < all.length; i++) {
                if (parentOf(all[i]) === row && all[i].alwaysVisible !== true)
                    return true;
            }
            return false;
        };
        var rows = [];
        for (var k = 0; k < all.length; k++) {
            var r = all[k];
            if (!isShown(r))
                continue;
            // A collapsed parent carries its hidden children's counts, so
            // the unread pill stays honest while they are folded away.
            var aggUnread = r.unread, aggTotal = r.count;
            var collapsible = canCollapse(r);
            if (collapsible && root.expandedById[r.id] !== true) {
                for (var m = 0; m < all.length; m++) {
                    var d = all[m];
                    if (d === r || isShown(d))
                        continue;
                    // Hidden offshoot of this row: walk up to confirm.
                    var q = d, under = false;
                    while (q !== null) {
                        if (q === r) {
                            under = true;
                            break;
                        }
                        q = parentOf(q);
                    }
                    if (under) {
                        aggUnread += d.unread;
                        aggTotal += d.count;
                    }
                }
            }
            rows.push({
                          id: r.id,
                          name: r.name,
                          role: r.role,
                          unread: r.unread,
                          count: r.count,
                          depth: r.depth,
                          leaf: r.leaf,
                          collapsible: collapsible,
                          expanded: root.expandedById[r.id] === true,
                          aggUnread: aggUnread,
                          aggTotal: aggTotal
                      });
        }
        ModelSync.sync(shown, rows, "name");
    }

    function toggleFolder(id) {
        var map = root.expandedById;
        if (map[id] === true)
            delete map[id];
        else
            map[id] = true;
        root.expandedById = map;
        root.refreshShown();
    }

    onFoldersChanged: root.refreshShown()

    // Switching account or folder rebuilds the models these delegates and the
    // account popup are built from, so the emit is deferred out of the click
    // handler (see MessageList.emitLater for the crash this avoids).
    function emitLater(sig, arg) {
        if (arg === undefined)
            Qt.callLater(sig);
        else
            Qt.callLater(sig, arg);
    }

    color: Theme.bgAlt

    Rectangle {
        anchors.right: parent.right
        width: 1
        height: parent.height
        color: Theme.border
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Account chip: shows who you are and switches between accounts.
        // Adding and managing accounts live in the toolbar's Accounts entry.
        ItemDelegate {
            id: accountChip
            readonly property bool canSwitch: !!root.accounts && root.accounts.count > 1
            // The current account's feed row, for its badge.
            readonly property var currentAccount: {
                var n = root.accounts ? root.accounts.count : 0;
                for (var i = 0; i < n; i++) {
                    if (root.accounts.get(i).email === root.currentEmail)
                        return root.accounts.get(i);
                }
                return null;
            }
            Layout.fillWidth: true
            Layout.margins: Theme.sm
            implicitHeight: 48
            hoverEnabled: canSwitch
            onClicked: {
                if (canSwitch)
                    accountMenu.popup();
            }

            background: Rectangle {
                radius: Theme.radius
                color: accountChip.hovered ? Theme.hover : "transparent"
                border.width: 1
                border.color: Theme.border
            }

            contentItem: RowLayout {
                spacing: Theme.sm
                Avatar {
                    implicitWidth: Math.round(28 * Theme.uiScale)
                    implicitHeight: Math.round(28 * Theme.uiScale)
                    badge: accountChip.currentAccount
                }
                ColumnLayout {
                    spacing: 0
                    Layout.fillWidth: true
                    Label {
                        text: root.currentEmail === "" ? qsTr("No account") : root.currentEmail
                        color: Theme.text
                        font.pixelSize: Theme.fontSmall
                        font.bold: true
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    Label {
                        visible: accountChip.canSwitch
                        text: qsTr("%1 accounts — switch").arg(root.accounts ? root.accounts.count : 0)
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontTiny
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }
                Label {
                    visible: accountChip.canSwitch
                    text: Icons.expandMore
                    font.family: Icons.fontFamily
                    color: Theme.textMuted
                }
            }

            AppMenu {
                id: accountMenu
                Repeater {
                    model: root.accounts
                    AppMenuItem {
                        required property var model
                        glyph: model.id === root.currentAccountId ? Icons.currentDot : ""
                        label: model.email
                        onTriggered: root.emitLater(root.accountSelected, model.id)
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Theme.md
            Layout.rightMargin: Theme.sm
            Layout.topMargin: Theme.sm
            Layout.bottomMargin: Theme.xs
            spacing: Theme.xs

            Label {
                Layout.fillWidth: true
                text: qsTr("FOLDERS")
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
                font.bold: true
                font.letterSpacing: 1
            }
        }

        ListView {
            id: folderList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: shown
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar {
                policy: ScrollBar.AsNeeded
            }

            delegate: ItemDelegate {
                id: folderRow
                width: folderList.width
                height: Theme.rowHeight

                required property var model
                readonly property bool current: folderRow.model.name === root.currentFolder

                onClicked: root.emitLater(root.folderSelected, folderRow.model.name)
                ToolTip.visible: folderRow.hovered
                ToolTip.text: qsTr("%1 total · %2 unread").arg(folderRow.model.aggTotal || 0).arg(
                                  folderRow.model.aggUnread)
                // Padding, not anchors: a control's contentItem is sized by
                // the control, so anchor margins inside it are ignored.
                // Indent follows the hierarchy depth, the way MoveTo does.
                leftPadding: Theme.md + Theme.sm + (folderRow.model.depth || 0) * 16
                rightPadding: Theme.md + Theme.sm

                background: Rectangle {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.sm
                    anchors.rightMargin: Theme.sm
                    radius: Theme.radius
                    color: folderRow.current ? Theme.selected : folderRow.hovered ? Theme.hover : "transparent"
                }

                contentItem: RowLayout {
                    spacing: Theme.sm

                    Label {
                        text: {
                            switch (folderRow.model.role) {
                            case "inbox":
                                return Icons.inbox;
                            case "drafts":
                                return Icons.drafts;
                            case "sent":
                                return Icons.send;
                            case "archive":
                                return Icons.archive;
                            case "junk":
                                return Icons.block;
                            case "trash":
                                return Icons.trash;
                            default:
                                return Icons.folder;
                            }
                        }
                        font.family: Icons.fontFamily
                        font.pixelSize: Theme.fontBase
                    }
                    Label {
                        text: folderRow.model.leaf
                        Layout.fillWidth: true
                        elide: Text.ElideRight
                        color: folderRow.current ? Theme.accent : Theme.text
                        font.pixelSize: Theme.fontBase
                        font.bold: (folderRow.model.aggUnread || 0) > 0
                    }
                    // Total cached, muted — with the unread pill next to it
                    // the row reads as "3 unread of 128". Both aggregate
                    // hidden children's counts while collapsed.
                    Label {
                        visible: (folderRow.model.aggTotal || 0) > 0
                        text: folderRow.model.aggTotal
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                    }
                    // Unread count as a pill, the way mail clients do it.
                    Rectangle {
                        visible: (folderRow.model.aggUnread || 0) > 0
                        implicitWidth: Math.max(Math.round(20 * Theme.uiScale), unreadLabel.implicitWidth + Theme.sm)
                        implicitHeight: Math.round(18 * Theme.uiScale)
                        radius: Math.round(9 * Theme.uiScale)
                        color: folderRow.current ? Theme.accent : Theme.border
                        Label {
                            id: unreadLabel
                            anchors.centerIn: parent
                            text: folderRow.model.aggUnread
                            color: folderRow.current ? Theme.accentText : Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                            font.bold: true
                        }
                    }
                    // Collapse chevron last, so the label edge never moves
                    // whether a row has children or not. Rows without
                    // children hold the same slot, keeping pills aligned.
                    Item {
                        visible: folderRow.model.collapsible !== true
                        Layout.preferredWidth: 24
                        Layout.preferredHeight: 1
                    }
                    IconButton {
                        visible: folderRow.model.collapsible === true
                        Layout.preferredWidth: 24
                        Layout.preferredHeight: 24
                        text: folderRow.model.expanded === true ? Icons.expandLess : Icons.expandMore
                        iconFont: true
                        fontSize: Theme.fontSmall
                        tooltip: folderRow.model.expanded === true ? qsTr("Collapse subfolders") : qsTr(
                                                                         "Expand subfolders")
                        onClicked: root.toggleFolder(folderRow.model.id)
                    }
                }
            }
        }

        // No folders yet: say what to do about it.
        Label {
            Layout.fillWidth: true
            Layout.margins: Theme.md
            visible: shown.count === 0
            text: root.currentEmail === "" ? qsTr("Add an account to begin.") : qsTr(
                                                 "No folders yet — press ⟳ to sync.")
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            wrapMode: Text.Wrap
        }
    }
}
