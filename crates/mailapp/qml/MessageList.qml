import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// Message list. Expects `messages` to be an array of plain objects with
// {uid, subject, from, date, snippet, unread, starred} (Main.qml's feed).
//
// Selection is keyed on UID, never on the row index (flaw F1). The old code
// bound `ListView.currentIndex` to a property, then reassigned it from a
// click and re-emitted selection from `onCurrentIndexChanged`; every feed
// reload cleared the model, which reset currentIndex, which fired selection
// again and dragged the highlight back to row 0. Now clicks report a UID
// upwards, the highlight is derived from `currentUid`, and a model rebuild
// cannot move the selection.
//
// Multi-select (Roundcube-style) lives alongside the preview selection:
// checkboxes fill `selectedUids` (also UID-keyed, pruned when the feed
// drops rows), the header box selects the visible page, and the bulk bar
// acts on the whole checkbox set in one backend call.
Rectangle {
    id: root

    // Plain JS array, not a ListModel: see qml/ModelSync.qml for why the feed
    // is never handed around as model-owned objects.
    property var messages: []
    property int currentUid: -1
    property string folderName: ""
    property string filterText: ""
    // Paging: `limit` is the backend page size (grows via "load older"),
    // `totalCount` the cached DB total for the folder. The footer shows when
    // the page is full and unfiltered — i.e. the server may hold more.
    property int totalCount: 0
    property int limit: 200
    property bool busy: false

    // Checkbox multi-selection (UIDs) + sort state (mirrors the backend's
    // persisted `message_sort_*` settings via Main). Checkboxes stay hidden
    // until `selectionMode` is toggled in the header, so the list reads
    // clean until a bulk action is actually wanted.
    property var selectedUids: []
    property int selectionVersion: 0
    property int lastClickedUid: -1
    property bool selectionMode: false
    property string sortField: "date"
    property bool sortDescending: true
    // "comfortable" (default) | "compact": compact tightens rows and hides
    // the preview line. Bound to the `list_density` setting via Main.
    property string density: "comfortable"

    signal messageSelected(int uid)
    signal starToggled(int uid)
    signal archiveRequested(int uid)
    signal moveRequested(int uid)
    signal markReadRequested(int uid, bool read)
    signal deleteRequested(int uid)
    signal purgeRequested(int uid)
    signal loadOlderRequested()
    signal bulkMarkReadRequested(var uids, bool read)
    signal bulkStarRequested(var uids, bool starred)
    signal bulkArchiveRequested(var uids)
    signal bulkMoveRequested(var uids)
    signal bulkDeleteRequested(var uids)
    signal bulkPurgeRequested(var uids)
    signal sortRequested(string field, bool descending)

    readonly property bool canLoadOlder: root.filterText === ""
        && root.messages.length > 0
        && root.messages.length >= root.limit

    // Row actions rebuild the feed, which destroys the delegates. Emitting
    // straight from a delegate's click handler therefore deletes the item
    // whose handler is still running -- a use-after-free that segfaulted the
    // app on delete. Qt.callLater defers the emit until the handler (and, for
    // the context menu, the popup close animation) has unwound.
    function emitLater(sig, uid) {
        Qt.callLater(sig, uid)
    }

    function emitLater2(sig, a, b) {
        Qt.callLater(sig, a, b)
    }

    // Context menu target. The menu lives here, not in the delegate: a popup
    // parented to a row would be destroyed underneath itself as the model
    // rebuilds.
    property int menuUid: -1
    property bool menuStarred: false
    property bool menuUnread: false

    color: Theme.bg

    // --- selection ------------------------------------------------------

    function isSelected(uid) {
        return root.selectedUids.indexOf(uid) !== -1
    }

    function setSelection(uids) {
        root.selectedUids = (uids || []).slice()
        root.selectionVersion++
    }

    function toggleSelection(uid) {
        var a = root.selectedUids.slice()
        var i = a.indexOf(uid)
        if (i === -1)
            a.push(uid)
        else
            a.splice(i, 1)
        root.lastClickedUid = uid
        root.setSelection(a)
    }

    function clearSelection() {
        if (root.selectedUids.length === 0)
            return
        root.setSelection([])
    }

    // Enter/exit checkbox selection mode. Exiting drops any selection so a
    // stale set can never act on the wrong folder afterwards.
    function setSelectionMode(on) {
        if (root.selectionMode === on)
            return
        root.selectionMode = on
        if (!on) {
            root.lastClickedUid = -1
            if (root.selectedUids.length > 0)
                root.setSelection([])
        }
    }

    function visibleUids() {
        var out = []
        for (var i = 0; i < filtered.count; i++)
            out.push(filtered.get(i).uid)
        return out
    }

    function selectAll() {
        root.setSelection(root.visibleUids())
    }

    function selectNone() {
        root.clearSelection()
    }

    function selectUnread() {
        var out = []
        for (var i = 0; i < filtered.count; i++) {
            var r = filtered.get(i)
            if (r.unread)
                out.push(r.uid)
        }
        root.setSelection(out)
    }

    function selectStarred() {
        var out = []
        for (var i = 0; i < filtered.count; i++) {
            var r = filtered.get(i)
            if (r.starred)
                out.push(r.uid)
        }
        root.setSelection(out)
    }

    function invertSelection() {
        var sel = {}
        for (var i = 0; i < root.selectedUids.length; i++)
            sel[root.selectedUids[i]] = true
        var out = []
        for (var j = 0; j < filtered.count; j++) {
            var uid = filtered.get(j).uid
            if (!sel[uid])
                out.push(uid)
        }
        root.setSelection(out)
    }

    // Shift-click range from the last clicked row to `toUid` (visible order).
    function selectRange(toUid) {
        if (filtered.count === 0)
            return
        var from = root.indexOfUid(root.lastClickedUid)
        var to = root.indexOfUid(toUid)
        if (from < 0 || to < 0) {
            root.toggleSelection(toUid)
            return
        }
        var lo = Math.min(from, to)
        var hi = Math.max(from, to)
        var sel = {}
        for (var i = 0; i < root.selectedUids.length; i++)
            sel[root.selectedUids[i]] = true
        for (var j = lo; j <= hi; j++)
            sel[filtered.get(j).uid] = true
        var out = []
        for (var k in sel)
            out.push(parseInt(k, 10))
        root.lastClickedUid = toUid
        root.setSelection(out)
    }

    function toggleSelectAll() {
        if (root.isAllSelected())
            root.clearSelection()
        else
            root.selectAll()
    }

    function isAllSelected() {
        if (filtered.count === 0)
            return false
        // Depend on the version so header/rows re-evaluate on every change.
        if (root.selectionVersion < 0)
            return false
        for (var i = 0; i < filtered.count; i++) {
            if (!root.isSelected(filtered.get(i).uid))
                return false
        }
        return true
    }

    function isNoneSelected() {
        if (root.selectionVersion < 0)
            return true
        return root.selectedUids.length === 0
    }

    function isPartialSelected() {
        return !root.isNoneSelected() && !root.isAllSelected()
    }

    // Drop checkbox UIDs the feed no longer carries (folder switch, delete,
    // move, sync expunge). Kept rows stay selected so mark/star can chain.
    function pruneSelection() {
        if (root.selectedUids.length === 0)
            return
        var live = {}
        var src = root.messages || []
        for (var i = 0; i < src.length; i++)
            live[src[i].uid] = true
        var kept = []
        for (var j = 0; j < root.selectedUids.length; j++) {
            if (live[root.selectedUids[j]])
                kept.push(root.selectedUids[j])
        }
        if (kept.length !== root.selectedUids.length)
            root.setSelection(kept)
    }

    // True when every selected row is starred (drives the Star/Unstar label).
    function selectionAllStarred() {
        if (root.selectedUids.length === 0)
            return false
        if (root.selectionVersion < 0)
            return false
        var byUid = {}
        var src = root.messages || []
        for (var i = 0; i < src.length; i++)
            byUid[src[i].uid] = src[i]
        for (var j = 0; j < root.selectedUids.length; j++) {
            var m = byUid[root.selectedUids[j]]
            if (m === undefined || !m.starred)
                return false
        }
        return true
    }

    // --- sorting --------------------------------------------------------

    function sortLabel() {
        var arrow = root.sortDescending ? "↓" : "↑"
        if (root.sortField === "from")
            return qsTr("From %1").arg(arrow)
        if (root.sortField === "subject")
            return qsTr("Subject %1").arg(arrow)
        return qsTr("Date %1").arg(arrow)
    }

    function sortTick(field, descending) {
        return (root.sortField === field && root.sortDescending === descending) ? "✓ " : ""
    }

    // Visible rows after applying the search filter.
    function matches(m) {
        if (root.filterText === "")
            return true
        var q = root.filterText.toLowerCase()
        return (m.subject || "").toLowerCase().indexOf(q) !== -1
            || (m.from || "").toLowerCase().indexOf(q) !== -1
            || (m.snippet || "").toLowerCase().indexOf(q) !== -1
    }

    // Only the roles a row actually draws. The feed also carries the full
    // bodies, which have no business in a list model.
    function displayRow(m) {
        return {
            uid: m.uid,
            subject: m.subject,
            from: m.from,
            date: m.date,
            snippet: m.snippet,
            unread: m.unread,
            starred: m.starred,
            has_attachments: m.has_attachments === true
        }
    }

    function rebuildFiltered() {
        var rows = []
        var src = root.messages || []
        for (var i = 0; i < src.length; i++) {
            if (root.matches(src[i]))
                rows.push(root.displayRow(src[i]))
        }
        // In place: clearing the model destroyed and rebuilt every delegate on
        // every click, including the one whose mouse handler was still running.
        ModelSync.sync(filtered, rows, "uid")
    }

    function indexOfUid(uid) {
        for (var i = 0; i < filtered.count; i++) {
            if (filtered.get(i).uid === uid)
                return i
        }
        return -1
    }

    // Move selection by one row (keyboard navigation from Main).
    function step(delta) {
        if (filtered.count === 0)
            return
        var i = root.indexOfUid(root.currentUid)
        var next = i < 0 ? 0 : Math.max(0, Math.min(filtered.count - 1, i + delta))
        root.messageSelected(filtered.get(next).uid)
        list.positionViewAtIndex(next, ListView.Contain)
    }

    ListModel {
        id: filtered
    }

    onMessagesChanged: {
        root.pruneSelection()
        root.scheduleRebuild()
    }
    onFilterTextChanged: root.scheduleRebuild()

    // Coalesced: a reload plus a filter keystroke in the same tick should cost
    // one rebuild, not two, and never one while a click handler is unwinding.
    function scheduleRebuild() {
        Qt.callLater(root.rebuildFiltered)
    }

    Column {
        anchors.fill: parent

        // Header: selection toggle, which folder, how many, sort + select menus.
        Rectangle {
            id: headerBar
            width: parent.width
            implicitHeight: 38 + ((root.selectionMode && root.selectedUids.length > 0) ? 40 : 0)
            color: Theme.bgAlt
            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: 1
                color: Theme.border
            }

            Column {
                anchors.fill: parent

                RowLayout {
                    width: parent.width
                    height: 38
                    spacing: 2

                    // Selection-mode toggle: checkboxes stay out of the way
                    // until bulk actions are actually wanted.
                    IconButton {
                        Layout.leftMargin: Theme.sm
                        text: "☑"
                        fontSize: Theme.fontSmall
                        active: root.selectionMode
                        tooltip: root.selectionMode ? qsTr("Hide selection checkboxes") : qsTr("Select messages")
                        onClicked: root.setSelectionMode(!root.selectionMode)
                    }

                    // Tri-state page checkbox (custom-drawn: a real CheckBox
                    // breaks its `checked` binding on the first click and then
                    // ignores programmatic select-all/clear).
                    Item {
                        Layout.preferredWidth: root.selectionMode ? 30 : 0
                        Layout.fillHeight: true
                        visible: root.selectionMode
                        Rectangle {
                            anchors.centerIn: parent
                            width: 18
                            height: 18
                            radius: Theme.xs
                            color: root.isAllSelected() ? Theme.accent
                                 : root.isNoneSelected() ? Theme.bg : Theme.bg
                            border.width: 1
                            border.color: root.isNoneSelected() ? Theme.border : Theme.accent
                            Text {
                                anchors.centerIn: parent
                                visible: root.isAllSelected()
                                text: "✓"
                                color: Theme.accentText
                                font.pixelSize: Theme.fontSmall
                                font.bold: true
                            }
                            Text {
                                anchors.centerIn: parent
                                visible: root.isPartialSelected()
                                text: "—"
                                color: Theme.accent
                                font.pixelSize: Theme.fontSmall
                                font.bold: true
                            }
                        }
                        MouseArea {
                            anchors.fill: parent
                            hoverEnabled: true
                            onClicked: root.toggleSelectAll()
                        }
                    }

                    Label {
                        Layout.fillWidth: true
                        text: root.folderName === "" ? qsTr("Messages") : root.folderName
                        color: Theme.text
                        font.pixelSize: Theme.fontBase
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    Label {
                        text: root.filterText === ""
                              ? qsTr("%1").arg(filtered.count)
                              : qsTr("%1 of %2").arg(filtered.count).arg(root.messages ? root.messages.length : 0)
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                    }
                    IconButton {
                        text: "⇅"
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Sort: %1").arg(root.sortLabel())
                        onClicked: sortMenu.popup()
                    }
                    Label {
                        text: root.sortLabel()
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontTiny
                        MouseArea {
                            anchors.fill: parent
                            onClicked: sortMenu.popup()
                        }
                    }
                    IconButton {
                        Layout.rightMargin: Theme.sm
                        visible: root.selectionMode
                        text: "▾"
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Select messages")
                        onClicked: selectMenu.popup()
                    }
                }

                // Bulk bar: Roundcube-style actions for the checkbox set.
                RowLayout {
                    visible: root.selectionMode && root.selectedUids.length > 0
                    width: parent.width
                    height: 40
                    spacing: 2

                    Label {
                        Layout.leftMargin: Theme.md
                        text: qsTr("%n selected", "", root.selectedUids.length)
                        color: Theme.text
                        font.pixelSize: Theme.fontSmall
                        font.bold: true
                    }
                    IconButton {
                        text: "✕"
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Clear selection")
                        onClicked: root.clearSelection()
                    }
                    Item { Layout.fillWidth: true }
                    IconButton {
                        text: "✓"
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Mark selected as read")
                        onClicked: root.emitLater2(root.bulkMarkReadRequested, root.selectedUids.slice(), true)
                    }
                    IconButton {
                        text: "○"
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Mark selected as unread")
                        onClicked: root.emitLater2(root.bulkMarkReadRequested, root.selectedUids.slice(), false)
                    }
                    IconButton {
                        text: root.selectionAllStarred() ? "☆" : "★"
                        fontSize: Theme.fontBase
                        contentColor: root.selectionAllStarred() ? Theme.textMuted : Theme.star
                        tooltip: root.selectionAllStarred() ? qsTr("Remove star from selected") : qsTr("Star selected")
                        onClicked: root.emitLater2(root.bulkStarRequested, root.selectedUids.slice(), !root.selectionAllStarred())
                    }
                    IconButton {
                        text: "🗄"
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Archive selected")
                        onClicked: {
                            var uids = root.selectedUids.slice()
                            Qt.callLater(root.bulkArchiveRequested, uids)
                        }
                    }
                    IconButton {
                        text: "➡"
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Move selected to…")
                        onClicked: {
                            var uids = root.selectedUids.slice()
                            Qt.callLater(root.bulkMoveRequested, uids)
                        }
                    }
                    IconButton {
                        text: "🗑"
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Move selected to Trash")
                        onClicked: {
                            var uids = root.selectedUids.slice()
                            Qt.callLater(root.bulkDeleteRequested, uids)
                        }
                    }
                    IconButton {
                        Layout.rightMargin: Theme.sm
                        text: "⋯"
                        fontSize: Theme.fontBase
                        tooltip: qsTr("More bulk actions")
                        onClicked: bulkMenu.popup()
                    }
                }
            }
        }

        ListView {
            id: list
            width: parent.width
            height: parent.height - headerBar.implicitHeight - (loadOlderBar.visible ? loadOlderBar.implicitHeight : 0)
            clip: true
            model: filtered
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

            delegate: Item {
                id: row
                width: list.width
                height: root.density === "compact" ? 58 : Theme.listItemHeight

                required property int index
                required property var model

                readonly property bool current: row.model.uid === root.currentUid
                readonly property bool checked: root.selectionVersion >= 0 && root.isSelected(row.model.uid)

                Rectangle {
                    anchors.fill: parent
                    color: row.checked ? Theme.selected
                         : row.current ? Theme.selected
                         : hoverArea.containsMouse ? Theme.hover
                         : "transparent"

                    // Accent bar marks the selected row without relying on
                    // ListView.isCurrentItem (which is only valid on the
                    // delegate root and silently did nothing in children).
                    Rectangle {
                        width: 3
                        height: parent.height
                        color: Theme.accent
                        visible: row.current || row.checked
                    }
                    Rectangle {
                        anchors.bottom: parent.bottom
                        width: parent.width
                        height: 1
                        color: Theme.border
                        opacity: 0.6
                    }
                }

                MouseArea {
                    id: hoverArea
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    onClicked: mouse => {
                        if (mouse.button === Qt.RightButton) {
                            root.menuUid = row.model.uid
                            root.menuStarred = row.model.starred
                            root.menuUnread = row.model.unread
                            rowMenu.popup()
                        } else if (mouse.modifiers & Qt.ControlModifier) {
                            if (!root.selectionMode)
                                root.setSelectionMode(true)
                            root.toggleSelection(row.model.uid)
                        } else if (mouse.modifiers & Qt.ShiftModifier) {
                            if (!root.selectionMode)
                                root.setSelectionMode(true)
                            root.selectRange(row.model.uid)
                            root.emitLater(root.messageSelected, row.model.uid)
                        } else {
                            root.lastClickedUid = row.model.uid
                            root.emitLater(root.messageSelected, row.model.uid)
                        }
                    }
                }

                Row {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.sm
                    anchors.rightMargin: Theme.sm
                    anchors.topMargin: Theme.sm
                    anchors.bottomMargin: Theme.sm
                    spacing: Theme.sm

                    // Checkbox cell (custom-drawn so programmatic
                    // select-all/clear always reflects — see header).
                    // Hidden until selection mode is toggled in the header.
                    Item {
                        id: checkCell
                        width: root.selectionMode ? 22 : 0
                        visible: root.selectionMode
                        height: parent.height
                        z: 1
                        Rectangle {
                            anchors.verticalCenter: parent.verticalCenter
                            width: 18
                            height: 18
                            radius: Theme.xs
                            color: row.checked ? Theme.accent : Theme.bg
                            border.width: 1
                            border.color: row.checked ? Theme.accent
                                        : hoverArea.containsMouse ? Theme.accent
                                        : Theme.border
                            Text {
                                anchors.centerIn: parent
                                visible: row.checked
                                text: "✓"
                                color: Theme.accentText
                                font.pixelSize: Theme.fontSmall
                                font.bold: true
                            }
                        }
                        MouseArea {
                            anchors.fill: parent
                            onClicked: root.toggleSelection(row.model.uid)
                        }
                    }

                    // Unread marker column.
                    Item {
                        width: 8
                        height: parent.height
                        Rectangle {
                            anchors.centerIn: parent
                            width: 8
                            height: 8
                            radius: 4
                            color: Theme.accent
                            visible: row.model.unread
                        }
                    }

                    Avatar {
                        anchors.verticalCenter: parent.verticalCenter
                        seed: row.model.from || "?"
                        initials: (row.model.from || "?").replace(/^[^a-zA-Z0-9]*/, "").substring(0, 1).toUpperCase()
                    }

                    Column {
                        width: parent.width - 8 - (root.selectionMode ? 22 : 0) - 34 - (Theme.sm * (root.selectionMode ? 4 : 3)) - 24
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 2

                        Row {
                            width: parent.width
                            spacing: Theme.sm
                            Label {
                                text: row.model.from || qsTr("(unknown sender)")
                                color: Theme.text
                                font.pixelSize: Theme.fontBase
                                font.bold: row.model.unread
                                elide: Text.ElideRight
                                width: parent.width - 66 - (row.model.has_attachments ? 18 : 0)
                            }
                            Label {
                                text: row.model.has_attachments ? "📎" : ""
                                color: Theme.textMuted
                                font.pixelSize: Theme.fontTiny
                                width: row.model.has_attachments ? 14 : 0
                                visible: row.model.has_attachments
                            }
                            Label {
                                text: row.model.date
                                color: Theme.textMuted
                                font.pixelSize: Theme.fontTiny
                                width: 58
                                horizontalAlignment: Text.AlignRight
                            }
                        }
                        Label {
                            text: row.model.subject
                            color: row.model.unread ? Theme.text : Theme.textMuted
                            font.pixelSize: Theme.fontBase
                            font.bold: row.model.unread
                            elide: Text.ElideRight
                            width: parent.width
                        }
                        Label {
                            visible: root.density !== "compact"
                            text: row.model.snippet
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontSmall
                            elide: Text.ElideRight
                            width: parent.width
                        }
                    }

                    // Star, always visible when set, on hover otherwise.
                    IconButton {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 24
                        height: 24
                        fontSize: Theme.fontBase
                        visible: row.model.starred || hoverArea.containsMouse
                        text: row.model.starred ? "★" : "☆"
                        contentColor: row.model.starred ? Theme.star : Theme.textMuted
                        tooltip: row.model.starred ? qsTr("Remove star") : qsTr("Star")
                        onClicked: root.emitLater(root.starToggled, row.model.uid)
                    }
                }
            }
        }

        // Paging footer: one batch (200) per press, fetched from the server
        // below the oldest cached UID, then appended to the feed. Hidden for
        // small folders and while filtering (the filter only sees loaded mail).
        Rectangle {
            id: loadOlderBar
            width: parent.width
            implicitHeight: root.canLoadOlder ? 56 : 0
            visible: root.canLoadOlder
            color: Theme.bgAlt
            clip: true
            Rectangle {
                anchors.top: parent.top
                width: parent.width
                height: 1
                color: Theme.border
            }
            Row {
                anchors.centerIn: parent
                spacing: Theme.sm
                Label {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.totalCount > root.messages.length
                          ? qsTr("%1 of %2 shown").arg(root.messages.length).arg(root.totalCount)
                          : qsTr("%1 shown").arg(root.messages.length)
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                }
                AppButton {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.busy ? qsTr("Loading…") : qsTr("Show older messages")
                    enabled: !root.busy
                    onClicked: root.loadOlderRequested()
                }
            }
        }
    }

    AppMenu {
        id: rowMenu

        MenuItem {
            text: root.menuUnread ? qsTr("Mark as read") : qsTr("Mark as unread")
            onTriggered: Qt.callLater(root.markReadRequested, root.menuUid, root.menuUnread)
        }
        MenuItem {
            text: root.menuStarred ? qsTr("Remove star") : qsTr("Star")
            onTriggered: root.emitLater(root.starToggled, root.menuUid)
        }
        MenuItem {
            text: qsTr("Archive")
            onTriggered: root.emitLater(root.archiveRequested, root.menuUid)
        }
        MenuItem {
            text: qsTr("Move to…")
            onTriggered: root.emitLater(root.moveRequested, root.menuUid)
        }
        MenuItem {
            text: qsTr("Move to Trash")
            onTriggered: root.emitLater(root.deleteRequested, root.menuUid)
        }
        MenuSeparator {}
        MenuItem {
            text: qsTr("Delete permanently…")
            onTriggered: root.emitLater(root.purgeRequested, root.menuUid)
        }
    }

    AppMenu {
        id: selectMenu

        MenuItem {
            text: qsTr("Select all visible")
            onTriggered: root.selectAll()
        }
        MenuItem {
            text: qsTr("Select none")
            onTriggered: root.selectNone()
        }
        MenuItem {
            text: qsTr("Select unread")
            onTriggered: root.selectUnread()
        }
        MenuItem {
            text: qsTr("Select starred")
            onTriggered: root.selectStarred()
        }
        MenuItem {
            text: qsTr("Invert selection")
            onTriggered: root.invertSelection()
        }
    }

    AppMenu {
        id: sortMenu

        MenuItem {
            text: root.sortTick("date", true) + qsTr("Date: newest first")
            onTriggered: root.sortRequested("date", true)
        }
        MenuItem {
            text: root.sortTick("date", false) + qsTr("Date: oldest first")
            onTriggered: root.sortRequested("date", false)
        }
        MenuSeparator {}
        MenuItem {
            text: root.sortTick("from", false) + qsTr("From: A to Z")
            onTriggered: root.sortRequested("from", false)
        }
        MenuItem {
            text: root.sortTick("from", true) + qsTr("From: Z to A")
            onTriggered: root.sortRequested("from", true)
        }
        MenuSeparator {}
        MenuItem {
            text: root.sortTick("subject", false) + qsTr("Subject: A to Z")
            onTriggered: root.sortRequested("subject", false)
        }
        MenuItem {
            text: root.sortTick("subject", true) + qsTr("Subject: Z to A")
            onTriggered: root.sortRequested("subject", true)
        }
    }

    AppMenu {
        id: bulkMenu

        MenuItem {
            text: qsTr("Select unread")
            onTriggered: root.selectUnread()
        }
        MenuItem {
            text: qsTr("Select starred")
            onTriggered: root.selectStarred()
        }
        MenuItem {
            text: qsTr("Clear selection")
            onTriggered: root.clearSelection()
        }
        MenuSeparator {}
        MenuItem {
            text: qsTr("Delete permanently…")
            onTriggered: {
                var uids = root.selectedUids.slice()
                Qt.callLater(root.bulkPurgeRequested, uids)
            }
        }
    }

    // Empty states, distinguishing "nothing here" from "nothing matched".
    Column {
        anchors.centerIn: parent
        spacing: Theme.sm
        visible: filtered.count === 0
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.filterText !== "" ? "🔍" : "📭"
            font.pixelSize: 32
            opacity: 0.5
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textMuted
            font.pixelSize: Theme.fontBase
            text: root.filterText !== "" ? qsTr("No message matches “%1”").arg(root.filterText)
                                          : qsTr("This folder is empty")
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            visible: root.filterText === ""
            text: qsTr("Press ⟳ to sync")
        }
    }
}
