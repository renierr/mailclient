import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import QtCore

import Mailclient
import "components"

// Message list. Expects `messages` to be an array of plain objects with
// {uid, subject, from, from_name, date, snippet, unread, starred}
// (Main.qml's feed).
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
// checkboxes fill `selectedKeys` (row keys: the UID in a folder, folder id
// plus UID for search hits; pruned when the feed drops rows), the header
// box selects the visible page, and the bulk bar acts on the whole checkbox
// set — one backend call per folder it spans.
Rectangle {
    id: root

    // Plain JS array, not a ListModel: see qml/ModelSync.qml for why the feed
    // is never handed around as model-owned objects.
    property var messages: []
    property int currentUid: -1
    property string folderName: ""
    property string filterText: ""
    // The app bridge, for the core's list filter (`search::filter_matches`).
    property var backend
    // Quick filters for the current list: each checked entry narrows the
    // visible rows (AND-combined). They apply in folder and search modes.
    property bool filterUnread: false
    property bool filterStarred: false
    property bool filterAttachments: false
    // Account-wide FTS mode: `searchRows` are index hits across every folder
    // (newest first, each with folder/folder_id), shown instead of the folder
    // feed. Opening a hit selects its folder and keeps the search; a row
    // action selects the hit's folder first (`searchFolderNeeded`), since
    // every bridge mutation is scoped to the selected folder.
    property bool searching: false
    property var searchRows: []
    // Similar messages mode: if non-empty, displays the dismissable chip
    property string similarSubject: ""
    // The folder a scoped search is limited to ("" = the whole account): its
    // hits all share one folder, so they get no section headers.
    property string searchFolder: ""
    // A server backfill for the current query is running.
    property bool serverSearching: false
    // `totalCount` is the local cache count; `serverTotal` is the count seen
    // during the last successful IMAP sync. All cached messages are displayed.
    property int totalCount: 0
    property int serverTotal: -1
    // The "Show older" state and whether loading can bring more, decided
    // by mailcore (`feed::older_state`).
    property string olderState: ""
    property bool olderCanLoad: false
    property int limit: 200
    property bool busy: false

    // Checkbox multi-selection (UIDs) + sort state (mirrors the backend's
    // persisted `message_sort_*` settings via Main). Checkboxes stay hidden
    // until `selectionMode` is toggled in the header, so the list reads
    // clean until a bulk action is actually wanted.
    property var selectedKeys: []
    property int selectionVersion: 0
    property string lastClickedKey: ""
    property bool selectionMode: false
    property string sortField: "date"
    property bool sortDescending: true
    // "comfortable" (default) | "compact": compact tightens rows and hides
    // the preview line. Bound to the `list_density` setting via Main.
    property string density: "comfortable"

    signal messageSelected(int uid)
    signal searchJump(string folderPath, int uid)
    // A search hit's row action needs its folder selected first (Main).
    signal searchFolderNeeded(string folderPath)
    signal findSimilarRequested(string folderPath, int uid)
    signal clearSimilarRequested
    signal starToggled(int uid)
    signal archiveRequested(int uid)
    signal moveRequested(int uid)
    signal markReadRequested(int uid, bool read)
    signal deleteRequested(int uid)
    signal purgeRequested(int uid)
    signal loadOlderRequested
    signal bulkMarkReadRequested(var uids, bool read)
    signal bulkStarRequested(var uids, bool starred)
    signal bulkArchiveRequested(var uids)
    signal bulkMoveRequested(var uids)
    signal bulkDeleteRequested(var uids)
    signal bulkPurgeRequested(var uids)
    signal sortRequested(string field, bool descending)
    signal statusMessage(string text)

    readonly property bool hasQuickFilter: root.filterUnread || root.filterStarred || root.filterAttachments
    readonly property bool hasAnyFilter: root.filterText !== "" || root.hasQuickFilter
    // Filters only narrow the loaded rows, so older mail stays reachable
    // under them (Flutter shows its Load older row the same way).
    readonly property bool canLoadOlder: root.folderName !== "" && root.olderCanLoad

    // Row actions rebuild the feed, which destroys the delegates. Emitting
    // straight from a delegate's click handler therefore deletes the item
    // whose handler is still running -- a use-after-free that segfaulted the
    // app on delete. Qt.callLater defers the emit until the handler (and, for
    // the context menu, the popup close animation) has unwound.
    function emitLater(sig, uid) {
        Qt.callLater(sig, uid);
    }

    function emitLater2(sig, a, b) {
        Qt.callLater(sig, a, b);
    }

    // Context menu target. The menu lives here, not in the delegate: a popup
    // parented to a row would be destroyed underneath itself as the model
    // rebuilds.
    property int menuUid: -1
    property string menuFolderPath: ""
    property bool menuStarred: false
    property bool menuUnread: false

    // The menu's UID, after making sure a search hit's folder is the one the
    // bridge acts on.
    function menuTarget() {
        if (root.searching)
            root.searchFolderNeeded(root.menuFolderPath);
        return root.menuUid;
    }

    color: Theme.bg

    // --- selection ------------------------------------------------------

    function isSelected(key) {
        return root.selectedKeys.indexOf(key) !== -1;
    }

    function setSelection(keys) {
        root.selectedKeys = (keys || []).slice();
        root.selectionVersion++;
    }

    function toggleSelection(key) {
        var a = root.selectedKeys.slice();
        var i = a.indexOf(key);
        if (i === -1)
            a.push(key);
        else
            a.splice(i, 1);
        root.lastClickedKey = key;
        root.setSelection(a);
    }

    function clearSelection() {
        if (root.selectedKeys.length === 0)
            return;
        root.setSelection([]);
    }

    // Enter/exit checkbox selection mode. Exiting drops any selection so a
    // stale set can never act on the wrong folder afterwards.
    function setSelectionMode(on) {
        if (root.selectionMode === on)
            return;
        root.selectionMode = on;
        if (!on) {
            root.lastClickedKey = "";
            if (root.selectedKeys.length > 0)
                root.setSelection([]);
        }
    }

    function keysWhere(pred) {
        var out = [];
        for (var i = 0; i < filtered.count; i++) {
            var r = filtered.get(i);
            if (pred(r))
                out.push(r.key);
        }
        return out;
    }

    function selectAll() {
        root.setSelection(root.keysWhere(r => true));
    }

    function selectNone() {
        root.clearSelection();
    }

    function selectUnread() {
        root.setSelection(root.keysWhere(r => r.unread));
    }

    function selectStarred() {
        root.setSelection(root.keysWhere(r => r.starred));
    }

    function invertSelection() {
        root.setSelection(root.keysWhere(r => !root.isSelected(r.key)));
    }

    function indexOfKey(key) {
        for (var i = 0; i < filtered.count; i++) {
            if (filtered.get(i).key === key)
                return i;
        }
        return -1;
    }

    // Shift-click range from the last clicked row to `toKey` (visible order).
    function selectRange(toKey) {
        if (filtered.count === 0)
            return;
        var from = root.indexOfKey(root.lastClickedKey);
        var to = root.indexOfKey(toKey);
        if (from < 0 || to < 0) {
            root.toggleSelection(toKey);
            return;
        }
        var lo = Math.min(from, to);
        var hi = Math.max(from, to);
        var out = root.selectedKeys.slice();
        for (var j = lo; j <= hi; j++) {
            var k = filtered.get(j).key;
            if (out.indexOf(k) === -1)
                out.push(k);
        }
        root.lastClickedKey = toKey;
        root.setSelection(out);
    }

    function toggleSelectAll() {
        if (root.isAllSelected())
            root.clearSelection();
        else
            root.selectAll();
    }

    function isAllSelected() {
        if (filtered.count === 0)
            return false;
        // Depend on the version so header/rows re-evaluate on every change.
        if (root.selectionVersion < 0)
            return false;
        for (var i = 0; i < filtered.count; i++) {
            if (!root.isSelected(filtered.get(i).key))
                return false;
        }
        return true;
    }

    function isNoneSelected() {
        if (root.selectionVersion < 0)
            return true;
        return root.selectedKeys.length === 0;
    }

    function isPartialSelected() {
        return !root.isNoneSelected() && !root.isAllSelected();
    }

    // The rows the checkbox set is drawn from, keyed like `displayRow`.
    function sourceByKey() {
        var out = {};
        if (root.searching) {
            var hits = root.searchRows || [];
            for (var i = 0; i < hits.length; i++)
                out[hits[i].folder_id + ":" + hits[i].uid] = hits[i];
        } else {
            var src = root.messages || [];
            for (var j = 0; j < src.length; j++)
                out[String(src[j].uid)] = src[j];
        }
        return out;
    }

    // Drop checkbox keys the feed no longer carries (folder switch, delete,
    // move, sync expunge). Kept rows stay selected so mark/star can chain.
    function pruneSelection() {
        if (root.selectedKeys.length === 0)
            return;
        var live = root.sourceByKey();
        var kept = root.selectedKeys.filter(k => live[k] !== undefined);
        if (kept.length !== root.selectedKeys.length)
            root.setSelection(kept);
    }

    // What the bulk signals carry: plain UIDs in a folder, {folder, uid}
    // for search hits (Main runs those folder by folder).
    function selectionTargets() {
        var byKey = root.sourceByKey();
        var out = [];
        for (var i = 0; i < root.selectedKeys.length; i++) {
            var m = byKey[root.selectedKeys[i]];
            if (m === undefined)
                continue;
            out.push(root.searching ? {
                                          folder: m.folder,
                                          uid: m.uid
                                      } : m.uid);
        }
        return out;
    }

    // True when every selected row is starred (drives the Star/Unstar label).
    function selectionAllStarred() {
        if (root.selectedKeys.length === 0)
            return false;
        if (root.selectionVersion < 0)
            return false;
        var byKey = root.sourceByKey();
        for (var j = 0; j < root.selectedKeys.length; j++) {
            var m = byKey[root.selectedKeys[j]];
            if (m === undefined || !m.starred)
                return false;
        }
        return true;
    }

    // --- sorting --------------------------------------------------------

    function sortLabel() {
        var arrow = root.sortDescending ? "↓" : "↑";
        if (root.sortField === "from")
            return qsTr("From %1").arg(arrow);
        if (root.sortField === "subject")
            return qsTr("Subject %1").arg(arrow);
        return qsTr("Date %1").arg(arrow);
    }

    function sortTick(field, descending) {
        return (root.sortField === field && root.sortDescending === descending) ? "✓ " : "";
    }

    function filterTick(on) {
        return on ? "✓ " : "";
    }

    // Quick filters alone (unread/starred/attachments): used for search
    // hits too, which the FTS query already matched, so the substring
    // filter must not run on them a second time.
    function matchesQuick(m) {
        if (root.filterUnread && !m.unread)
            return false;
        if (root.filterStarred && !m.starred)
            return false;
        if (root.filterAttachments && m.has_attachments !== true)
            return false;
        return true;
    }

    // Visible rows after applying the search filter.
    function matches(m) {
        if (!root.matchesQuick(m))
            return false;
        if (root.filterText === "" || !root.backend)
            return true;
        return root.backend.search_filter_matches(root.filterText, m.subject || "", m.from || "", m.from_name || "",
                                                  m.snippet || "");
    }

    // Only the roles a row actually draws. `sender` is the sent display
    // name, falling back to the address for mail without one (and for
    // search hits, whose feed carries no name). `key` is the ModelSync identity:
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
            sender: (m.from_name || "") !== "" ? m.from_name : m.from,
            date: root.displayDate(m),
            snippet: m.snippet,
            unread: m.unread,
            starred: m.starred,
            has_attachments: m.has_attachments === true,
            initials: m.initials || "",
            avatar_light: m.avatar_light || "",
            avatar_dark: m.avatar_dark || ""
        };
    }

    // A feed date is a clock time or a date except for one case, which is a
    // word. mailcore is Qt-free and has no catalogue, so it flags that case
    // in `date_key` and the word is translated here (see feed::ShortDate).
    function displayDate(m) {
        return m.date_key === "yesterday" ? qsTr("Yesterday") : m.date;
    }

    // Empty states, distinguishing "still looking" from "nothing matched"
    // and from "nothing here".
    function emptyText() {
        if (root.searching) {
            if (root.serverSearching)
                return qsTr("Searching the server…");
            if (root.hasQuickFilter && (root.searchRows || []).length > 0)
                return qsTr("No match survives this filter");
            return qsTr("No matches for “%1”").arg(root.filterText.trim());
        }
        if (root.hasQuickFilter && root.filterText === "")
            return qsTr("No message matches this filter");
        if (root.filterText !== "")
            return qsTr("No message matches “%1”").arg(root.filterText);
        return qsTr("This folder is empty");
    }

    function rebuildFiltered() {
        var rows = [];
        if (root.searching) {
            // The feed arrives grouped by folder for the section headers
            // (mailcore `feed::search_json`): folders in the order of their
            // newest hit, newest first inside each.
            var hits = root.searchRows || [];
            for (var i = 0; i < hits.length; i++) {
                if (!root.matchesQuick(hits[i]))
                    continue;
                hits[i].key = hits[i].folder_id + ":" + hits[i].uid;
                rows.push(root.displayRow(hits[i]));
            }
            ModelSync.sync(filtered, rows, "key");
            return;
        }
        var src = root.messages || [];
        for (var j = 0; j < src.length; j++) {
            if (root.matches(src[j]))
                rows.push(root.displayRow(src[j]));
        }
        // In place: clearing the model destroyed and rebuilt every delegate on
        // every click, including the one whose mouse handler was still running.
        // The row shuffle scrolls the view as a side effect; it must not
        // overwrite the remembered position it is about to restore.
        root.scrollRestoring = true;
        ModelSync.sync(filtered, rows, "uid");
        root.scrollRestoring = false;
        root.restoreScroll();
    }

    // Scroll memory per folder: the top visible row (by UID) and how far past
    // its top the view sits. The list comes back where it was after the
    // reader took its place (medium/narrow layouts hide the list), after a
    // feed rebuild (delete, mark read, new mail, load older) and on returning
    // to a folder. Anchored on a UID rather than a pixel offset, so rows that
    // appear above or vanish do not shift what the user was looking at; a
    // deleted anchor falls back to the row now at its index. A list resting
    // at the very top stays there, so new mail shows.
    property var scrollMemory: ({})
    property bool scrollRestoring: false

    function rememberScroll() {
        if (root.scrollRestoring || !root.visible || root.searching || root.folderName === "" || filtered.count === 0)
            return;
        var idx = list.indexAt(0, list.contentY);
        if (idx < 0)
            return;
        var item = list.itemAtIndex(idx);
        root.scrollMemory[root.folderName] = {
            uid: filtered.get(idx).uid,
            index: idx,
            offset: item ? list.contentY - item.y : 0,
            atTop: list.atYBeginning
        };
    }

    function restoreScroll() {
        if (root.searching || filtered.count === 0)
            return;
        var mem = root.scrollMemory[root.folderName];
        root.scrollRestoring = true;
        if (mem === undefined || mem.atTop) {
            list.positionViewAtBeginning();
        } else {
            var idx = root.indexOfUid(mem.uid);
            var offset = mem.offset;
            if (idx < 0) {
                idx = Math.min(mem.index, filtered.count - 1);
                offset = 0;
            }
            list.positionViewAtIndex(idx, ListView.Beginning);
            var maxY = list.originY + Math.max(0, list.contentHeight - list.height);
            list.contentY = Math.min(list.contentY + offset, maxY);
        }
        root.scrollRestoring = false;
    }

    function indexOfUid(uid) {
        for (var i = 0; i < filtered.count; i++) {
            if (filtered.get(i).uid === uid)
                return i;
        }
        return -1;
    }

    // Move selection by one row (keyboard navigation from Main). In search
    // mode rows belong to foreign folders, so stepping jumps instead of
    // selecting in place.
    function step(delta) {
        if (filtered.count === 0)
            return;
        if (root.searching) {
            var k = 0;
            for (var i = 0; i < filtered.count; i++) {
                if (filtered.get(i).uid === root.currentUid) {
                    k = i;
                    break;
                }
            }
            var next = Math.max(0, Math.min(filtered.count - 1, k + delta));
            var row = filtered.get(next);
            root.searchJump(row.folder, row.uid);
            list.positionViewAtIndex(next, ListView.Contain);
            return;
        }
        var at = root.indexOfUid(root.currentUid);
        var following = at < 0 ? 0 : Math.max(0, Math.min(filtered.count - 1, at + delta));
        root.messageSelected(filtered.get(following).uid);
    }

    ListModel {
        id: filtered
    }

    onMessagesChanged: {
        root.pruneSelection();
        root.scheduleRebuild();
    }
    onFilterTextChanged: root.scheduleRebuild()
    onFilterUnreadChanged: root.scheduleRebuild()
    onFilterStarredChanged: root.scheduleRebuild()
    onFilterAttachmentsChanged: root.scheduleRebuild()
    onSearchRowsChanged: {
        root.pruneSelection();
        root.scheduleRebuild();
    }
    onVisibleChanged: {
        if (root.visible)
            Qt.callLater(root.restoreScroll);
    }
    onSearchingChanged: {
        // A checkbox set belongs to the list it was made in: carried across,
        // its keys would point at the wrong rows.
        root.setSelectionMode(false);
        root.scheduleRebuild();
    }

    // Coalesced: a reload plus a filter keystroke in the same tick should cost
    // one rebuild, not two, and never one while a click handler is unwinding.
    function scheduleRebuild() {
        Qt.callLater(root.rebuildFiltered);
    }

    Column {
        anchors.fill: parent

        // Header: selection toggle, which folder, how many, sort + select menus.
        Rectangle {
            id: headerBar
            width: parent.width
            implicitHeight: headerCol.implicitHeight
            color: Theme.bgAlt
            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: 1
                color: Theme.border
            }

            Column {
                id: headerCol
                width: parent.width

                RowLayout {
                    width: parent.width
                    height: 38
                    spacing: 2

                    // Selection-mode toggle: checkboxes stay out of the way
                    // until bulk actions are actually wanted.
                    IconButton {
                        Layout.leftMargin: Theme.xs
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
                            color: root.isAllSelected() ? Theme.accent : root.isNoneSelected() ? Theme.bg : Theme.bg
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
                        text: root.similarSubject !== "" ? qsTr("Similar messages") : root.searching ? qsTr(
                                                                                                           "Search results") :
                                                                                                       root.folderName
                                                                                                       === "" ? qsTr(
                                                                                                                    "Messages") :
                                                                                                                root.folderName
                        color: Theme.text
                        font.pixelSize: Theme.fontBase
                        font.bold: true
                        elide: Text.ElideRight
                    }
                    Label {
                        text: root.similarSubject !== "" ? qsTr("%n result(s)", "", filtered.count) : root.searching ? (
                                                                                                                           root.searchFolder
                                                                                                                           !== "" ? qsTr(
                                                                                                                                        "%n result(s) in %1",
                                                                                                                                        "", filtered.count).arg(
                                                                                                                                        root.searchFolder) :
                                                                                                                                    qsTr("%n result(s) across this account",
                                                                                                                                         "", filtered.count)) :
                                                                                                                       !root.hasAnyFilter
                                                                                                                       ? qsTr("%1").arg(
                                                                                                                             filtered.count) :
                                                                                                                         qsTr("%1 of %2").arg(
                                                                                                                             filtered.count).arg(
                                                                                                                             root.messages
                                                                                                                             ? root.messages.length :
                                                                                                                               0)
                        color: Theme.textMuted
                        font.pixelSize: Theme.fontSmall
                    }
                    IconButton {
                        text: Icons.filterList
                        iconFont: true
                        fontSize: Theme.fontSmall
                        active: root.hasQuickFilter
                        tooltip: root.hasQuickFilter ? qsTr("Filter: active") : qsTr("Filter messages")
                        onClicked: filterMenu.popup()
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
                        visible: root.selectionMode
                        text: Icons.expandMore
                        iconFont: true
                        fontSize: Theme.fontSmall
                        tooltip: qsTr("Select messages")
                        onClicked: selectMenu.popup()
                    }
                }

                // Dismissable chip for "Find similar" search
                Rectangle {
                    visible: root.similarSubject !== ""
                    width: parent.width
                    height: visible ? Math.round(28 * Theme.uiScale) : 0
                    color: "transparent"

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: Theme.sm
                        anchors.rightMargin: Theme.sm
                        spacing: Theme.xs

                        Rectangle {
                            Layout.fillWidth: false
                            Layout.maximumWidth: parent.width
                            height: Math.round(24 * Theme.uiScale)
                            radius: Math.round(12 * Theme.uiScale)
                            color: Theme.selected
                            border.color: Theme.accent
                            border.width: 1

                            RowLayout {
                                anchors.fill: parent
                                anchors.leftMargin: Theme.sm
                                anchors.rightMargin: Theme.xs
                                spacing: Theme.xs

                                Label {
                                    text: Icons.search
                                    font.family: Icons.fontFamily
                                    font.pixelSize: Theme.fontTiny
                                    color: Theme.accent
                                }
                                Label {
                                    Layout.maximumWidth: Math.round(260 * Theme.uiScale)
                                    text: qsTr("Similar to: %1").arg(root.similarSubject)
                                    color: Theme.text
                                    font.pixelSize: Theme.fontSmall
                                    elide: Text.ElideRight
                                }
                                IconButton {
                                    width: Math.round(18 * Theme.uiScale)
                                    height: Math.round(18 * Theme.uiScale)
                                    fontSize: Theme.fontTiny
                                    text: Icons.close
                                    iconFont: true
                                    contentColor: Theme.textMuted
                                    tooltip: qsTr("Clear similar search")
                                    onClicked: root.clearSimilarRequested()
                                }
                            }
                        }
                        Item {
                            Layout.fillWidth: true
                        }
                    }
                }

                // Quick filter chips: Unread, Starred, Attachments
                Rectangle {
                    id: filterChipsRow
                    width: parent.width
                    height: Math.round(28 * Theme.uiScale)
                    color: "transparent"

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: Theme.sm
                        anchors.rightMargin: Theme.sm
                        spacing: Theme.xs

                        // Unread chip
                        Rectangle {
                            height: Math.round(22 * Theme.uiScale)
                            radius: Math.round(11 * Theme.uiScale)
                            color: root.filterUnread ? Theme.selected : "transparent"
                            border.color: root.filterUnread ? Theme.accent : Theme.border
                            border.width: 1
                            implicitWidth: unreadRow.implicitWidth + Math.round(16 * Theme.uiScale)

                            RowLayout {
                                id: unreadRow
                                anchors.centerIn: parent
                                spacing: 4

                                Label {
                                    text: Icons.markUnread
                                    font.family: Icons.fontFamily
                                    font.pixelSize: Theme.fontTiny
                                    color: root.filterUnread ? Theme.accent : Theme.textMuted
                                }
                                Label {
                                    text: qsTr("Unread")
                                    font.pixelSize: Theme.fontTiny
                                    font.bold: root.filterUnread
                                    color: root.filterUnread ? Theme.text : Theme.textMuted
                                }
                            }

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.filterUnread = !root.filterUnread
                            }
                        }

                        // Starred chip
                        Rectangle {
                            height: Math.round(22 * Theme.uiScale)
                            radius: Math.round(11 * Theme.uiScale)
                            color: root.filterStarred ? Theme.selected : "transparent"
                            border.color: root.filterStarred ? Theme.accent : Theme.border
                            border.width: 1
                            implicitWidth: starredRow.implicitWidth + Math.round(16 * Theme.uiScale)

                            RowLayout {
                                id: starredRow
                                anchors.centerIn: parent
                                spacing: 4

                                Label {
                                    text: Icons.star
                                    font.family: Icons.fontFamily
                                    font.pixelSize: Theme.fontTiny
                                    color: root.filterStarred ? Theme.accent : Theme.textMuted
                                }
                                Label {
                                    text: qsTr("Starred")
                                    font.pixelSize: Theme.fontTiny
                                    font.bold: root.filterStarred
                                    color: root.filterStarred ? Theme.text : Theme.textMuted
                                }
                            }

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.filterStarred = !root.filterStarred
                            }
                        }

                        // Attachments chip
                        Rectangle {
                            height: Math.round(22 * Theme.uiScale)
                            radius: Math.round(11 * Theme.uiScale)
                            color: root.filterAttachments ? Theme.selected : "transparent"
                            border.color: root.filterAttachments ? Theme.accent : Theme.border
                            border.width: 1
                            implicitWidth: attachmentsRow.implicitWidth + Math.round(16 * Theme.uiScale)

                            RowLayout {
                                id: attachmentsRow
                                anchors.centerIn: parent
                                spacing: 4

                                Label {
                                    text: Icons.attachFile
                                    font.family: Icons.fontFamily
                                    font.pixelSize: Theme.fontTiny
                                    color: root.filterAttachments ? Theme.accent : Theme.textMuted
                                }
                                Label {
                                    text: qsTr("Attachments")
                                    font.pixelSize: Theme.fontTiny
                                    font.bold: root.filterAttachments
                                    color: root.filterAttachments ? Theme.text : Theme.textMuted
                                }
                            }

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.filterAttachments = !root.filterAttachments
                            }
                        }

                        // Clear action when any quick filter is active
                        Label {
                            visible: root.hasQuickFilter
                            text: qsTr("Clear")
                            font.pixelSize: Theme.fontTiny
                            color: Theme.accent
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    root.filterUnread = false;
                                    root.filterStarred = false;
                                    root.filterAttachments = false;
                                }
                            }
                        }

                        Item {
                            Layout.fillWidth: true
                        }
                    }
                }

                // Bulk bar: Roundcube-style actions for the checkbox set.
                BulkActionBar {
                    visible: root.selectionMode && root.selectedKeys.length > 0
                    implicitHeight: visible ? Math.round(40 * Theme.uiScale) : 0
                    width: parent.width
                    selectedCount: root.selectedKeys.length
                    allStarred: root.selectionAllStarred()
                    onClearRequested: root.clearSelection()
                    onMarkReadRequested: root.emitLater2(root.bulkMarkReadRequested, root.selectionTargets(), true)
                    onMarkUnreadRequested: root.emitLater2(root.bulkMarkReadRequested, root.selectionTargets(), false)
                    onToggleStarRequested: root.emitLater2(root.bulkStarRequested, root.selectionTargets(),
                                                           !root.selectionAllStarred())
                    onArchiveRequested: {
                        var uids = root.selectionTargets();
                        Qt.callLater(root.bulkArchiveRequested, uids);
                    }
                    onMoveRequested: {
                        var uids = root.selectionTargets();
                        Qt.callLater(root.bulkMoveRequested, uids);
                    }
                    onDeleteRequested: {
                        var uids = root.selectionTargets();
                        Qt.callLater(root.bulkDeleteRequested, uids);
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
            onContentYChanged: root.rememberScroll()
            // Account-wide hits live in many folders: one header per folder.
            section.property: root.searching && root.searchFolder === "" ? "folder" : ""
            section.delegate: Rectangle {
                required property string section
                width: list.width
                height: Math.round(24 * Theme.uiScale)
                color: Theme.bgAlt
                Label {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.sm
                    anchors.rightMargin: Theme.sm
                    verticalAlignment: Text.AlignVCenter
                    text: parent.section
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    font.bold: true
                    elide: Text.ElideRight
                }
            }
            ScrollBar.vertical: ScrollBar {
                policy: ScrollBar.AsNeeded
            }

            delegate: Item {
                id: row
                width: list.width
                height: root.density === "compact" ? Math.round(58 * Theme.uiScale) : Theme.listItemHeight

                required property int index
                required property var model

                // Search hits span folders: a UID is only unique within one.
                readonly property bool current: row.model.uid === root.currentUid && (!root.searching
                                                                                      || row.model.folder
                                                                                      === root.folderName)
                readonly property bool checked: root.selectionVersion >= 0 && root.isSelected(row.model.key)

                Rectangle {
                    anchors.fill: parent
                    color: row.checked ? Theme.selected : row.current ? Theme.selected : hoverArea.containsMouse
                                                                        ? Theme.hover : "transparent"

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
                            root.menuUid = row.model.uid;
                            root.menuFolderPath = row.model.folder !== undefined ? row.model.folder : root.folderName;
                            root.menuStarred = row.model.starred;
                            root.menuUnread = row.model.unread;
                            rowMenu.popup();
                        } else if (mouse.modifiers & Qt.ControlModifier) {
                            if (!root.selectionMode)
                                root.setSelectionMode(true);
                            root.toggleSelection(row.model.key);
                        } else if (mouse.modifiers & Qt.ShiftModifier) {
                            if (!root.selectionMode)
                                root.setSelectionMode(true);
                            root.selectRange(row.model.key);
                            if (!root.searching)
                                root.emitLater(root.messageSelected, row.model.uid);
                        } else if (root.searching) {
                            root.lastClickedKey = row.model.key;
                            root.emitLater2(root.searchJump, row.model.folder, row.model.uid);
                        } else {
                            root.lastClickedKey = row.model.key;
                            root.emitLater(root.messageSelected, row.model.uid);
                        }
                    }
                }

                // Tight at the pane edge so sender and subject keep every
                // pixel: narrow side margins, small avatar, small gaps.
                Row {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.xs
                    anchors.rightMargin: Theme.xs
                    anchors.topMargin: Theme.xs
                    anchors.bottomMargin: Theme.xs
                    spacing: Theme.xs

                    // Avatar slot: in selection mode the checkbox takes the
                    // avatar's place instead of a column of its own, so the
                    // sender/subject keep their width. Custom-drawn so
                    // programmatic select-all/clear always reflects (see header).
                    // The avatar sits at the top of the row; the paperclip
                    // below it (not inline before the text) marks attachments.
                    Item {
                        id: checkCell
                        width: Math.round(28 * Theme.uiScale)
                        height: parent.height
                        z: 1

                        Avatar {
                            id: avatar
                            anchors.top: parent.top
                            anchors.horizontalCenter: parent.horizontalCenter
                            anchors.topMargin: 2
                            visible: !root.selectionMode
                            implicitWidth: Math.round(26 * Theme.uiScale)
                            implicitHeight: Math.round(26 * Theme.uiScale)
                            badge: row.model
                        }
                        Rectangle {
                            anchors.top: parent.top
                            anchors.horizontalCenter: parent.horizontalCenter
                            anchors.topMargin: 4
                            visible: root.selectionMode
                            width: Theme.checkSize
                            height: Theme.checkSize
                            radius: Theme.xs
                            color: row.checked ? Theme.accent : Theme.bg
                            border.width: 1
                            border.color: row.checked ? Theme.accent : hoverArea.containsMouse ? Theme.accent :
                                                                                                 Theme.border

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
                        // Attachment cue under the avatar, not inline before
                        // the text: the sender/subject keep the full width.
                        // The gap scales with the interface like the avatar
                        // and the glyph it separates (unlike layout spacing,
                        // which stays put): a fixed gap collapses visually
                        // at 125/150%.
                        Label {
                            anchors.top: avatar.bottom
                            anchors.horizontalCenter: parent.horizontalCenter
                            anchors.topMargin: Math.round(6 * Theme.uiScale)
                            text: Icons.attachFile
                            font.family: Icons.fontFamily
                            color: Theme.textMuted
                            font.pixelSize: Theme.fontTiny
                            visible: row.model.has_attachments
                        }
                        // Unread marker as a badge on the avatar's corner, not
                        // a column of its own: the row starts at the pane edge.
                        Rectangle {
                            x: avatar.x - 1
                            y: avatar.y - 1
                            width: 10
                            height: 10
                            radius: 5
                            color: Theme.accent
                            border.width: 2
                            border.color: Theme.bg
                            visible: row.model.unread && !root.selectionMode
                        }
                        MouseArea {
                            anchors.fill: parent
                            enabled: root.selectionMode
                            onClicked: root.toggleSelection(row.model.key)
                        }
                    }

                    Column {
                        id: textCell
                        width: parent.width - checkCell.width - Theme.xs * 3
                        anchors.top: parent.top
                        anchors.topMargin: 1
                        spacing: 2

                        Row {
                            id: fromRow
                            width: parent.width
                            spacing: Theme.xs
                            // Cue glyphs render at the scaled tiny font, so
                            // their cells scale too; the sender takes exactly
                            // what the visible cells and gaps leave over.
                            readonly property int cueWidth: Math.round(14 * Theme.uiScale)
                            // The sent display name (address only where the
                            // mail carries none), always with the compact
                            // date at the top right.
                            Label {
                                text: row.model.sender || row.model.from || qsTr("(unknown sender)")
                                color: Theme.text
                                font.pixelSize: Theme.fontBase
                                font.bold: row.model.unread
                                elide: Text.ElideRight
                                width: Math.max(0, parent.width - dateLabel.width - Theme.xs - (row.model.starred
                                                                                                ? fromRow.cueWidth
                                                                                                  + Theme.xs : 0))
                            }
                            // Passive starred cue (the toggle lives in the row
                            // menu now, so a starred row still reads starred).
                            Label {
                                text: row.model.starred ? Icons.star : ""
                                font.family: Icons.fontFamily
                                color: Theme.star
                                font.pixelSize: Theme.fontTiny
                                width: fromRow.cueWidth
                                visible: row.model.starred
                            }
                            Label {
                                id: dateLabel
                                text: row.model.date
                                color: Theme.textMuted
                                font.pixelSize: Theme.fontTiny
                                width: Math.round(58 * Theme.uiScale)
                                horizontalAlignment: Text.AlignRight
                            }
                        }
                        // Subject row: the text yields to the ⋮ menu, which
                        // sits one line below the date at the same edge.
                        Row {
                            width: parent.width
                            spacing: Theme.xs
                            Label {
                                text: row.model.subject
                                // Full text colour like the sender line (and
                                // the Flutter row): unread reads bold, not dim.
                                color: Theme.text
                                font.pixelSize: Theme.fontBase
                                font.bold: row.model.unread
                                elide: Text.ElideRight
                                // The ⋮ slot is always reserved, so revealing
                                // the button on hover reflows nothing.
                                width: Math.max(0, parent.width - Theme.miniButton - Theme.xs)
                            }
                            // Row actions menu (⋮), opening the same menu as
                            // right-click: mark read/unread, star, archive,
                            // move, trash, purge (plus Open in search mode).
                            // Always visible, like the Flutter row — no
                            // hover-reveal, so it is always reachable and
                            // nothing ever reflows.
                            IconButton {
                                id: moreButton
                                width: Theme.miniButton
                                height: Theme.miniButton
                                fontSize: Theme.fontBase
                                text: Icons.moreVert
                                iconFont: true
                                contentColor: Theme.textMuted
                                tooltip: qsTr("Message actions")
                                onClicked: {
                                    root.menuUid = row.model.uid;
                                    root.menuFolderPath = row.model.folder !== undefined ? row.model.folder :
                                                                                           root.folderName;
                                    root.menuStarred = row.model.starred;
                                    root.menuUnread = row.model.unread;
                                    rowMenu.popup();
                                }
                            }
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
                    }
                }
            }
        }

        // One batch (200) per press: the newest server mail the cache lacks.
        // Only shown when older mail remains or when the server has not been checked.
        Rectangle {
            id: loadOlderBar
            width: parent.width
            // Hidden for an empty folder the server agrees is empty: the
            // list's own empty text says so.
            implicitHeight: root.folderName !== "" && root.olderState !== "" && root.olderState !== "empty" ? 56 : 0
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
                            return "";
                        var s;
                        if (root.olderState === "unchecked")
                            s = qsTr("Cached %1 (server not checked)").arg(root.totalCount);
                        else if (root.olderState === "partial")
                            s = qsTr("Cached %1 of %2").arg(root.totalCount).arg(root.serverTotal);
                        else
                            return qsTr("All %1 messages loaded").arg(root.totalCount);
                        return root.hasAnyFilter ? qsTr("%1 · filters cover loaded mail only").arg(s) : s;
                    }
                    color: Theme.textMuted
                    font.pixelSize: Theme.fontSmall
                    maximumLineCount: 1
                }
                AppButton {
                    id: loadOlderButton
                    Layout.alignment: Qt.AlignVCenter
                    visible: root.canLoadOlder
                    text: root.busy ? qsTr("Loading…") : (root.olderState === "unchecked" ? qsTr("Check server") : qsTr(
                                                                                                "Load older"))
                    enabled: !root.busy
                    onClicked: root.loadOlderRequested()
                }
            }
        }
    }

    AppMenu {
        id: rowMenu

        // Search hits belong to foreign folders: `menuTarget` has Main select
        // the hit's folder first, so the folder-scoped actions hit the right
        // message (the results stay on screen).
        AppMenuItem {
            visible: root.searching
            glyph: Icons.mail
            label: qsTr("Open message")
            onTriggered: Qt.callLater(root.searchJump, root.menuFolderPath, root.menuUid)
        }
        MenuSeparator {
            visible: root.searching
        }
        AppMenuItem {
            glyph: root.menuUnread ? Icons.markRead : Icons.markUnread
            label: root.menuUnread ? qsTr("Mark as read") : qsTr("Mark as unread")
            onTriggered: Qt.callLater(root.markReadRequested, root.menuTarget(), root.menuUnread)
        }
        AppMenuItem {
            glyph: root.menuStarred ? Icons.starBorder : Icons.star
            label: root.menuStarred ? qsTr("Remove star") : qsTr("Star")
            onTriggered: root.emitLater(root.starToggled, root.menuTarget())
        }
        AppMenuItem {
            glyph: Icons.archive
            label: qsTr("Archive")
            onTriggered: root.emitLater(root.archiveRequested, root.menuTarget())
        }
        AppMenuItem {
            glyph: Icons.driveFileMove
            label: qsTr("Move to…")
            onTriggered: root.emitLater(root.moveRequested, root.menuTarget())
        }
        AppMenuItem {
            glyph: Icons.trash
            label: qsTr("Move to Trash")
            onTriggered: root.emitLater(root.deleteRequested, root.menuTarget())
        }
        MenuSeparator {}
        AppMenuItem {
            glyph: Icons.search
            label: qsTr("Find similar")
            onTriggered: root.emitLater2(root.findSimilarRequested, root.menuFolderPath, root.menuUid)
        }
        AppMenuItem {
            glyph: Icons.fileDownload
            label: qsTr("Save as .eml…")
            onTriggered: root.exportEml(root.menuFolderPath, root.menuUid)
        }
        AppMenuItem {
            glyph: Icons.deleteForever
            label: qsTr("Delete permanently…")
            onTriggered: root.emitLater(root.purgeRequested, root.menuTarget())
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
        id: filterMenu

        // Each entry toggles one quick filter; several can stay on at once
        // (AND-combined). The tick column marks what is active.
        AppMenuItem {
            glyph: Icons.markUnread
            label: root.filterTick(root.filterUnread) + qsTr("Unread only")
            onTriggered: root.filterUnread = !root.filterUnread
        }
        AppMenuItem {
            glyph: Icons.star
            label: root.filterTick(root.filterStarred) + qsTr("Starred only")
            onTriggered: root.filterStarred = !root.filterStarred
        }
        AppMenuItem {
            glyph: Icons.attachFile
            label: root.filterTick(root.filterAttachments) + qsTr("With attachments")
            onTriggered: root.filterAttachments = !root.filterAttachments
        }
        MenuSeparator {
            visible: root.hasQuickFilter
        }
        AppMenuItem {
            visible: root.hasQuickFilter
            glyph: Icons.clear
            label: qsTr("Clear filters")
            onTriggered: {
                root.filterUnread = false;
                root.filterStarred = false;
                root.filterAttachments = false;
            }
        }
    }

    AppMenu {
        id: bulkMenu

        // Complete action set: the bulk bar collapses buttons into here on
        // narrow panes, so every bar action must have a menu twin.
        AppMenuItem {
            glyph: Icons.markRead
            label: qsTr("Mark selected as read")
            onTriggered: root.emitLater2(root.bulkMarkReadRequested, root.selectionTargets(), true)
        }
        AppMenuItem {
            glyph: Icons.markUnread
            label: qsTr("Mark selected as unread")
            onTriggered: root.emitLater2(root.bulkMarkReadRequested, root.selectionTargets(), false)
        }
        AppMenuItem {
            glyph: root.selectionAllStarred() ? Icons.starBorder : Icons.star
            label: root.selectionAllStarred() ? qsTr("Remove star from selected") : qsTr("Star selected")
            onTriggered: root.emitLater2(root.bulkStarRequested, root.selectionTargets(), !root.selectionAllStarred())
        }
        AppMenuItem {
            glyph: Icons.archive
            label: qsTr("Archive selected")
            onTriggered: {
                var uids = root.selectionTargets();
                Qt.callLater(root.bulkArchiveRequested, uids);
            }
        }
        AppMenuItem {
            glyph: Icons.driveFileMove
            label: qsTr("Move selected to…")
            onTriggered: {
                var uids = root.selectionTargets();
                Qt.callLater(root.bulkMoveRequested, uids);
            }
        }
        AppMenuItem {
            glyph: Icons.trash
            label: qsTr("Move selected to Trash")
            onTriggered: {
                var uids = root.selectionTargets();
                Qt.callLater(root.bulkDeleteRequested, uids);
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
                var uids = root.selectionTargets();
                Qt.callLater(root.bulkPurgeRequested, uids);
            }
        }
    }

    // Over the list's bottom-right corner (the list sits in the Column at
    // the root's origin, so its geometry is root-relative).
    ScrollJumpButtons {
        target: list
        x: list.x + list.width - width - Theme.md
        y: list.y + list.height - height - Theme.sm
    }

    // Empty states, distinguishing "nothing here" from "nothing matched".
    Column {
        anchors.centerIn: parent
        spacing: Theme.sm
        visible: filtered.count === 0
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.hasAnyFilter ? Icons.search : Icons.inbox
            font.family: Icons.fontFamily
            font.pixelSize: 32
            opacity: 0.5
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textMuted
            font.pixelSize: Theme.fontBase
            text: root.emptyText()
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            color: Theme.textMuted
            font.pixelSize: Theme.fontSmall
            visible: !root.hasAnyFilter
            text: qsTr("Press ⟳ to sync")
        }
    }

    FileDialog {
        id: exportEmlDialog
        title: qsTr("Save as .eml")
        fileMode: FileDialog.SaveFile
        nameFilters: [qsTr("Mail files (*.eml)"), qsTr("All files (*)")]
        property string exportFolderPath: ""
        property int exportUid: -1
        onAccepted: {
            if (root.backend && root.backend.export_message) {
                root.statusMessage(qsTr("Exporting…"));
                var r = root.backend.export_message(-1, exportUid, selectedFile.toString());
                if (r !== "")
                    root.statusMessage(r);
            }
        }
    }

    function exportEml(folderPath, uid) {
        if (!root.backend || uid < 0)
            return;
        exportEmlDialog.exportFolderPath = folderPath;
        exportEmlDialog.exportUid = uid;
        var name = root.backend.suggested_eml_name ? root.backend.suggested_eml_name(-1, uid) : ("message-" + uid
                                                                                                 + ".eml");
        var base = StandardPaths.writableLocation(StandardPaths.DownloadLocation);
        var s = base.toString().replace(/\\/g, "/");
        if (s.indexOf("file:") !== 0) {
            if (s.length >= 2 && s[1] === ":")
                s = "/" + s;
            s = "file://" + s;
        }
        s = s.replace(/\/+$/, "") + "/" + encodeURIComponent(name);
        exportEmlDialog.selectedFile = s;
        exportEmlDialog.open();
    }
}
