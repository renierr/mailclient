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
    // Account-wide FTS mode: `searchRows` are index hits across every folder
    // (rank order, each with folder/folder_id), shown instead of the folder
    // feed. Rows are navigation-only here — every mutation below is scoped
    // to the current folder, so acting on a foreign UID would hit the wrong
    // message. Selecting a hit jumps to its folder and clears the search.
    property bool searching: false
    property var searchRows: []
    // `totalCount` is the local cache count; `serverTotal` is the count seen
    // during the last successful IMAP sync. All cached messages are displayed.
    property int totalCount: 0
    property int serverTotal: -1
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
    signal searchJump(string folderPath, int uid)
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

    readonly property bool canLoadOlder: root.folderName !== ""
        && root.filterText === ""
        && (root.messages.length > 0 || root.serverTotal < 0)
        && (root.serverTotal < 0 || root.serverTotal > root.totalCount)

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
    property string menuFolderPath: ""
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

    // Only the roles a row actually draws. `key` is the ModelSync identity:
    // plain UIDs in folder mode, folder-scoped keys for search hits (the
    // same UID can hit in several folders at once). Always a string: the
    // model is shared between modes and ListModel roles are typed by first
    // assignment (mixed Number/String logs "Can't assign to existing role").
    function displayRow(m) {
        return {
            key: m.key !== undefined ? String(m.key) : String(m.uid),
            uid: m.uid,
            folder: m.folder || "",
            subject: m.subject,
            from: m.from,
            date: root.displayDate(m),
            snippet: m.snippet,
            unread: m.unread,
            starred: m.starred,
            has_attachments: m.has_attachments === true
        }
    }

    // A feed date is a clock time or a date except for one case, which is a
    // word. mailcore is Qt-free and has no catalogue, so it flags that case
    // in `date_key` and the word is translated here (see feed::ShortDate).
    function displayDate(m) {
        return m.date_key === "yesterday" ? qsTr("Yesterday") : m.date
    }

    function rebuildFiltered() {
        var rows = []
        if (root.searching) {
            var hits = root.searchRows || []
            for (var i = 0; i < hits.length; i++) {
                hits[i].key = hits[i].folder_id + ":" + hits[i].uid
                rows.push(root.displayRow(hits[i]))
            }
            ModelSync.sync(filtered, rows, "key")
            return
        }
        var src = root.messages || []
        for (var j = 0; j < src.length; j++) {
            if (root.matches(src[j]))
                rows.push(root.displayRow(src[j]))
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

    // Move selection by one row (keyboard navigation from Main). In search
    // mode rows belong to foreign folders, so stepping jumps instead of
    // selecting in place.
    function step(delta) {
        if (filtered.count === 0)
            return
        if (root.searching) {
            var k = 0
            for (var i = 0; i < filtered.count; i++) {
                if (filtered.get(i).uid === root.currentUid) {
                    k = i
                    break
                }
            }
            var next = Math.max(0, Math.min(filtered.count - 1, k + delta))
            var row = filtered.get(next)
            root.searchJump(row.folder, row.uid)
            list.positionViewAtIndex(next, ListView.Contain)
            return
        }
        var at = root.indexOfUid(root.currentUid)
        var following = at < 0 ? 0 : Math.max(0, Math.min(filtered.count - 1, at + delta))
        root.messageSelected(filtered.get(following).uid)
    }

    ListModel {
        id: filtered
    }

    onMessagesChanged: {
        root.pruneSelection()
        root.scheduleRebuild()
    }
    onFilterTextChanged: root.scheduleRebuild()
    onSearchRowsChanged: root.scheduleRebuild()
    onSearchingChanged: {
        // Search results are navigation-only: a stale checkbox set must
        // never act on foreign-folder UIDs afterwards.
        if (root.searching)
            root.setSelectionMode(false)
        root.scheduleRebuild()
    }

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
                    // until bulk actions are actually wanted. Hidden while
                    // searching (results are navigation-only).
                    IconButton {
                        visible: !root.searching
                        Layout.leftMargin: Theme.sm
                        text: root.selectionMode ? Icons.checkBox : Icons.checkBoxBlank
                        iconFont: true
                        fontSize: Theme.fontSmall
                        active: root.selectionMode
                        tooltip: root.selectionMode ? qsTr("Hide selection checkboxes") : qsTr("Select messages")
                        onClicked: root.setSelectionMode(!root.selectionMode)
                    }

                    // Tri-state page checkbox (custom-drawn: a real CheckBox
                    // breaks its `checked` binding on the first click and then
                    // ignores programmatic select-all/clear).
                    Item {
                        Layout.preferredWidth: root.selectionMode ? Math.round(30 * Theme.uiScale) : 0
                        Layout.fillHeight: true
                        visible: root.selectionMode
                        Rectangle {
                            anchors.centerIn: parent
                            width: Theme.checkSize
                            height: Theme.checkSize
                            radius: Theme.xs
                            color: root.isAllSelected() ? Theme.accent
                                 : root.isNoneSelected() ? Theme.bg : Theme.bg
                            border.width: 1
                            border.color: root.isNoneSelected() ? Theme.border : Theme.accent
                            Text {
                                anchors.centerIn: parent
                                visible: root.isAllSelected()
                                text: Icons.done
                                font.family: Icons.fontFamily
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
                        text: root.searching ? qsTr("Search results") : root.folderName === "" ? qsTr("Messages") : root.folderName
                        color: Theme.text
                        font.pixelSize: Theme.fontBase
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    Label {
                        text: root.searching
                              ? qsTr("%n result(s) across this account", "", filtered.count)
                              : root.filterText === ""
                                ? qsTr("%1").arg(filtered.count)
                                : qsTr("%1 of %2").arg(filtered.count).arg(root.messages ? root.messages.length : 0)
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                    }
                    IconButton {
                        visible: !root.searching
                        text: Icons.sort
                        iconFont: true
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Sort: %1").arg(root.sortLabel())
                        onClicked: sortMenu.popup()
                    }
                    Label {
                        visible: !root.searching
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
                        visible: root.selectionMode && !root.searching
                        text: Icons.expandMore
                        iconFont: true
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Select messages")
                        onClicked: selectMenu.popup()
                    }
                }

                // Bulk bar: Roundcube-style actions for the checkbox set.
                BulkActionBar {
                    visible: root.selectionMode && root.selectedUids.length > 0
                    width: parent.width
                    selectedCount: root.selectedUids.length
                    allStarred: root.selectionAllStarred()
                    onClearRequested: root.clearSelection()
                    onMarkReadRequested: root.emitLater2(root.bulkMarkReadRequested, root.selectedUids.slice(), true)
                    onMarkUnreadRequested: root.emitLater2(root.bulkMarkReadRequested, root.selectedUids.slice(), false)
                    onToggleStarRequested: root.emitLater2(root.bulkStarRequested, root.selectedUids.slice(), !root.selectionAllStarred())
                    onArchiveRequested: {
                        var uids = root.selectedUids.slice()
                        Qt.callLater(root.bulkArchiveRequested, uids)
                    }
                    onMoveRequested: {
                        var uids = root.selectedUids.slice()
                        Qt.callLater(root.bulkMoveRequested, uids)
                    }
                    onDeleteRequested: {
                        var uids = root.selectedUids.slice()
                        Qt.callLater(root.bulkDeleteRequested, uids)
                    }
                    onMoreRequested: bulkMenu.popup()
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
                height: (root.density === "compact" ? Math.round(58 * Theme.uiScale) : Theme.listItemHeight) + (root.searching ? Math.round(18 * Theme.uiScale) : 0)

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
                        if (root.searching) {
                            // Navigation-only: every mutation below is scoped
                            // to the current folder.
                            if (mouse.button === Qt.RightButton) {
                                root.menuUid = row.model.uid
                                root.menuFolderPath = row.model.folder
                                rowMenu.popup()
                            } else {
                                root.emitLater2(root.searchJump, row.model.folder, row.model.uid)
                            }
                            return
                        }
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
                        width: root.selectionMode ? Math.round(22 * Theme.uiScale) : 0
                        visible: root.selectionMode
                        height: parent.height
                        z: 1
                        Rectangle {
                            anchors.verticalCenter: parent.verticalCenter
                            width: Theme.checkSize
                            height: Theme.checkSize
                            radius: Theme.xs
                            color: row.checked ? Theme.accent : Theme.bg
                            border.width: 1
                            border.color: row.checked ? Theme.accent
                                        : hoverArea.containsMouse ? Theme.accent
                                        : Theme.border
                            Text {
                                anchors.centerIn: parent
                                visible: row.checked
                                text: Icons.done
                                font.family: Icons.fontFamily
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
                        width: parent.width - 8 - (root.selectionMode ? Math.round(22 * Theme.uiScale) : 0) - Math.round(34 * Theme.uiScale) - (Theme.sm * (root.selectionMode ? 4 : 3)) - Theme.miniButton
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
                                width: parent.width - Math.round(66 * Theme.uiScale) - (row.model.has_attachments ? Math.round(18 * Theme.uiScale) : 0)
                            }
                            Label {
                                text: row.model.has_attachments ? Icons.attachFile : ""
                                font.family: Icons.fontFamily
                                color: Theme.textMuted
                                font.pixelSize: Theme.fontTiny
                                width: row.model.has_attachments ? 14 : 0
                                visible: row.model.has_attachments
                            }
                            Label {
                                text: row.model.date
                                color: Theme.textMuted
                                font.pixelSize: Theme.fontTiny
                                width: Math.round(58 * Theme.uiScale)
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
                        maximumLineCount: 1
                        width: parent.width
                    }
                    // Search hits live in foreign folders: say which one.
                    Label {
                        visible: root.searching
                        text: row.model.folder || ""
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontTiny
                        elide: Text.ElideRight
                        width: parent.width
                    }
                    }

                    // Star, always visible when set, on hover otherwise.
                    IconButton {
                        anchors.verticalCenter: parent.verticalCenter
                        width: Theme.miniButton
                        height: Theme.miniButton
                        fontSize: Theme.fontBase
                        visible: row.model.starred || hoverArea.containsMouse
                        text: row.model.starred ? Icons.star : Icons.starBorder
                        iconFont: true
                        contentColor: row.model.starred ? Theme.star : Theme.textMuted
                        tooltip: row.model.starred ? qsTr("Remove star") : qsTr("Star")
                        onClicked: {
                            if (root.searching)
                                root.emitLater2(root.searchJump, row.model.folder, row.model.uid)
                            else
                                root.emitLater(root.starToggled, row.model.uid)
                        }
                    }
                }
            }
        }

        // One batch (200) per press, fetched below the oldest cached UID.
        // Only shown when older mail remains or when the server has not been checked.
        Rectangle {
            id: loadOlderBar
            width: parent.width
            implicitHeight: root.folderName !== "" && root.filterText === ""
                && (root.messages.length > 0 || root.serverTotal < 0) ? 56 : 0
            visible: implicitHeight > 0
            color: Theme.bgAlt
            clip: true
            Rectangle {
                anchors.top: parent.top
                width: parent.width
                height: 1
                color: Theme.border
            }
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Theme.md
                anchors.rightMargin: Theme.md
                spacing: Theme.sm
                Label {
                    id: olderStatus
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignVCenter
                    elide: Text.ElideRight
                    text: {
                        if (root.folderName === "")
                            return ""
                        if (root.serverTotal < 0)
                            return qsTr("Cached %1 (server not checked)").arg(root.totalCount)
                        if (root.serverTotal > root.totalCount)
                            return qsTr("Cached %1 of %2").arg(root.totalCount).arg(root.serverTotal)
                        if (root.messages.length === 0)
                            return qsTr("No cached messages")
                        return qsTr("All %1 messages loaded").arg(root.totalCount)
                    }
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    maximumLineCount: 1
                }
                AppButton {
                    id: loadOlderButton
                    Layout.alignment: Qt.AlignVCenter
                    visible: root.canLoadOlder
                    text: root.busy
                          ? qsTr("Loading…")
                          : (root.serverTotal < 0 ? qsTr("Check server") : qsTr("Load older"))
                    enabled: !root.busy
                    onClicked: root.loadOlderRequested()
                }
            }
        }
    }

    AppMenu {
        id: rowMenu

        // Search hits belong to foreign folders: the folder-scoped actions
        // below would hit the wrong message, so search mode only opens.
        AppMenuItem {
            visible: root.searching
            glyph: Icons.mail
            label: qsTr("Open message")
            onTriggered: Qt.callLater(root.searchJump, root.menuFolderPath, root.menuUid)
        }
        AppMenuItem {
            visible: !root.searching
            glyph: root.menuUnread ? Icons.markRead : Icons.markUnread
            label: root.menuUnread ? qsTr("Mark as read") : qsTr("Mark as unread")
            onTriggered: Qt.callLater(root.markReadRequested, root.menuUid, root.menuUnread)
        }
        AppMenuItem {
            visible: !root.searching
            glyph: root.menuStarred ? Icons.starBorder : Icons.star
            label: root.menuStarred ? qsTr("Remove star") : qsTr("Star")
            onTriggered: root.emitLater(root.starToggled, root.menuUid)
        }
        AppMenuItem {
            visible: !root.searching
            glyph: Icons.archive
            label: qsTr("Archive")
            onTriggered: root.emitLater(root.archiveRequested, root.menuUid)
        }
        AppMenuItem {
            visible: !root.searching
            glyph: Icons.driveFileMove
            label: qsTr("Move to…")
            onTriggered: root.emitLater(root.moveRequested, root.menuUid)
        }
        AppMenuItem {
            visible: !root.searching
            glyph: Icons.trash
            label: qsTr("Move to Trash")
            onTriggered: root.emitLater(root.deleteRequested, root.menuUid)
        }
        MenuSeparator { visible: !root.searching }
        AppMenuItem {
            visible: !root.searching
            glyph: Icons.deleteForever
            label: qsTr("Delete permanently…")
            onTriggered: root.emitLater(root.purgeRequested, root.menuUid)
        }
    }

    AppMenu {
        id: selectMenu

        AppMenuItem {
            glyph: Icons.selectAll
            label: qsTr("Select all visible")
            onTriggered: root.selectAll()
        }
        AppMenuItem {
            glyph: Icons.deselect
            label: qsTr("Select none")
            onTriggered: root.selectNone()
        }
        AppMenuItem {
            glyph: Icons.markUnread
            label: qsTr("Select unread")
            onTriggered: root.selectUnread()
        }
        AppMenuItem {
            glyph: Icons.star
            label: qsTr("Select starred")
            onTriggered: root.selectStarred()
        }
        AppMenuItem {
            glyph: Icons.swapHoriz
            label: qsTr("Invert selection")
            onTriggered: root.invertSelection()
        }
    }

    AppMenu {
        id: sortMenu

        // No glyphs here on purpose: the tick column already marks the
        // active sort, and an icon per row would fight it.
        AppMenuItem {
            label: root.sortTick("date", true) + qsTr("Date: newest first")
            onTriggered: root.sortRequested("date", true)
        }
        AppMenuItem {
            label: root.sortTick("date", false) + qsTr("Date: oldest first")
            onTriggered: root.sortRequested("date", false)
        }
        MenuSeparator {}
        AppMenuItem {
            label: root.sortTick("from", false) + qsTr("From: A to Z")
            onTriggered: root.sortRequested("from", false)
        }
        AppMenuItem {
            label: root.sortTick("from", true) + qsTr("From: Z to A")
            onTriggered: root.sortRequested("from", true)
        }
        MenuSeparator {}
        AppMenuItem {
            label: root.sortTick("subject", false) + qsTr("Subject: A to Z")
            onTriggered: root.sortRequested("subject", false)
        }
        AppMenuItem {
            label: root.sortTick("subject", true) + qsTr("Subject: Z to A")
            onTriggered: root.sortRequested("subject", true)
        }
    }

    AppMenu {
        id: bulkMenu

        // Complete action set: the bulk bar collapses buttons into here on
        // narrow panes, so every bar action must have a menu twin.
        AppMenuItem {
            glyph: Icons.markRead
            label: qsTr("Mark selected as read")
            onTriggered: root.emitLater2(root.bulkMarkReadRequested, root.selectedUids.slice(), true)
        }
        AppMenuItem {
            glyph: Icons.markUnread
            label: qsTr("Mark selected as unread")
            onTriggered: root.emitLater2(root.bulkMarkReadRequested, root.selectedUids.slice(), false)
        }
        AppMenuItem {
            glyph: root.selectionAllStarred() ? Icons.starBorder : Icons.star
            label: root.selectionAllStarred() ? qsTr("Remove star from selected") : qsTr("Star selected")
            onTriggered: root.emitLater2(root.bulkStarRequested, root.selectedUids.slice(), !root.selectionAllStarred())
        }
        AppMenuItem {
            glyph: Icons.archive
            label: qsTr("Archive selected")
            onTriggered: {
                var uids = root.selectedUids.slice()
                Qt.callLater(root.bulkArchiveRequested, uids)
            }
        }
        AppMenuItem {
            glyph: Icons.driveFileMove
            label: qsTr("Move selected to…")
            onTriggered: {
                var uids = root.selectedUids.slice()
                Qt.callLater(root.bulkMoveRequested, uids)
            }
        }
        AppMenuItem {
            glyph: Icons.trash
            label: qsTr("Move selected to Trash")
            onTriggered: {
                var uids = root.selectedUids.slice()
                Qt.callLater(root.bulkDeleteRequested, uids)
            }
        }
        MenuSeparator {}
        AppMenuItem {
            glyph: Icons.markUnread
            label: qsTr("Select unread")
            onTriggered: root.selectUnread()
        }
        AppMenuItem {
            glyph: Icons.star
            label: qsTr("Select starred")
            onTriggered: root.selectStarred()
        }
        AppMenuItem {
            glyph: Icons.clear
            label: qsTr("Clear selection")
            onTriggered: root.clearSelection()
        }
        MenuSeparator {}
        AppMenuItem {
            glyph: Icons.deleteForever
            label: qsTr("Delete permanently…")
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
            text: root.filterText !== "" ? Icons.search : Icons.inbox
            font.family: Icons.fontFamily
            font.pixelSize: 32
            opacity: 0.5
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textMuted
            font.pixelSize: Theme.fontBase
            text: root.searching ? qsTr("No matches in this account")
                  : root.filterText !== "" ? qsTr("No message matches “%1”").arg(root.filterText)
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
