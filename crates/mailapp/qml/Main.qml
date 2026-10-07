import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Mailclient
import "components"

// App shell: 3-pane mail layout (sidebar / list / reader).
// Data comes from the Rust Bridge (SQLite + IMAP); no mock models remain.
//
// Selection is held as a UID (`currentUid`), not a row index. Indices break
// the moment the feed is rebuilt — which happens after every open, star,
// delete and sync — and that was the cause of flaw F1.
ApplicationWindow {
    id: root
    visible: true
    // Never open larger than the screen actually offers: at 150% scaling a
    // 1320x860 logical window is ~1980x1290 physical, which does not fit a
    // 1080p laptop and pushes the reader pane off the edge.
    width: Math.min(1320, Screen.desktopAvailableWidth - 80)
    height: Math.min(860, Screen.desktopAvailableHeight - 80)
    minimumWidth: 380
    minimumHeight: 460
    title: qsTr("Mailclient")
    color: Theme.bg

    // The bundled vector icon font. Loaded once, application-global from
    // then on. The file sits next to this one, so the relative source
    // resolves in embedded, dist and dev runs alike (the dist copy mirrors
    // qml/ one to one, and the same path is embedded via qrc_resources).
    FontLoader {
        id: iconFontLoader
        source: "fonts/MaterialIcons-Regular.ttf"
    }

    // Pooled IMAP sessions stay logged in between actions — drop them on
    // quit (no LOGOUT round-trip, so this never blocks on a dead line).
    onClosing: backend.disconnect_all()

    // Inherited by every control in the window. The wrapped components in
    // components/ paint themselves from Theme, but ScrollBar, ToolTip, text
    // selection and dialog overlays are drawn by the style -- without a
    // palette they use its light defaults and read as a different app.
    palette.window: Theme.bg
    palette.windowText: Theme.text
    palette.base: Theme.bg
    palette.alternateBase: Theme.bgAlt
    palette.button: Theme.bgRaised
    palette.buttonText: Theme.text
    palette.text: Theme.text
    palette.placeholderText: Theme.textMuted
    palette.mid: Theme.border
    palette.midlight: Theme.border
    palette.dark: Theme.border
    palette.highlight: Theme.accent
    palette.highlightedText: Theme.accentText
    palette.toolTipBase: Theme.bgRaised
    palette.toolTipText: Theme.text

    property string currentFolder: ""
    property int currentUid: -1
    property string statusText: qsTr("Starting…")
    property bool busy: backend.busy
    property bool readerFullscreen: false

    // --- responsive panes -------------------------------------------------
    // Wide shows all three panes; medium pairs the sidebar with the list or
    // the reader (whichever the selection calls for); narrow shows one pane
    // at a time and navigates between them. The manual sidebar toggle only
    // exists where there is a choice — the wide layout.
    readonly property bool wideLayout: root.width >= 1100
    readonly property bool mediumLayout: root.width >= 720 && root.width < 1100
    property bool sidebarOpen: true
    // Narrow navigation: folders | list | reader. A cold start opens where
    // the `start_view` setting says (Component.onCompleted).
    property string narrowPane: "list"
    // Toolbar controls grow with the interface scale but the window minimum
    // does not, so below this width the secondary actions fold into a menu.
    readonly property bool compactToolbar: root.width < Math.round(700 * Theme.uiScale)

    function toggleReaderFullscreen() {
        if (!root.readerFullscreen && (root.currentUid < 0 || root.currentMessage === undefined))
            return;
        root.readerFullscreen = !root.readerFullscreen;
    }

    // Fullscreen hides everything except the reader — including the exit
    // control, which lives inside the message pane. If the open mail
    // disappears while maximized (delete/archive/move/purge), drop back to
    // the 3-pane view instead of stranding the user in an empty fullscreen
    // pane that no button (and no Esc reaching past WebEngine focus) can leave.
    onCurrentMessageChanged: {
        if (root.currentMessage === undefined && root.readerFullscreen)
            root.readerFullscreen = false;
        // Same trap in the narrow layout: the reader's Back button lives in
        // the message header, so an emptied reader pane has no way out.
        if (root.currentMessage === undefined && root.narrowPane === "reader")
            root.narrowPane = "list";
    }

    // Leave the reader (medium/narrow Back): drop the selection, so the same
    // row opens again on the next click, and cancel a pending delayed
    // mark-as-read, since the user stopped viewing before it elapsed.
    function closeReader() {
        markReadTimer.stop();
        root.currentUid = -1;
        root.currentMessage = undefined;
        root.narrowPane = "list";
    }

    Bridge {
        id: backend
    }

    SettingsBridge {
        id: appSettings
    }

    // Interface scale: the SettingsBridge owns the value, Theme owns the
    // rendering — this keeps them in sync, live on every Save.
    Binding {
        target: Theme
        property: "uiScale"
        value: appSettings.ui_scale
    }

    ListModel {
        id: folderModel
    }
    ListModel {
        id: accountModel
    }

    // The message feed is kept as plain JavaScript objects, not a ListModel.
    // `ListModel.get()` hands out QObjects the model owns, so the reader pane
    // holding the selected message was dereferencing freed memory the moment a
    // reload cleared the model -- the segfault on repeated clicks. Plain
    // objects are snapshots and stay valid.
    property var messageRows: []
    // Compact list rows are cheap to swap between folders. The selected mail's
    // body and attachment records are fetched separately, on demand.
    property var currentMessage: undefined

    // Account-wide FTS results (newest first) for the toolbar search. Short
    // input keeps the instant current-folder substring filter; 3+ letters
    // query the index across every folder of the account instead.
    property var searchRows: []
    property bool searching: false
    // A server SEARCH backfill is in flight (thin local hits are topped up
    // from IMAP, then the local re-query picks them up).
    property bool serverSearching: false
    // Query the last backfill ran for: the same text is never searched
    // twice, so a finished job cannot re-arm another one (the loop).
    property string lastServerQuery: ""
    // How the search field runs its text, decided by mailcore
    // (`search::plan`: mode, trimmed query, hit limit, debounce).
    property var searchPlan: ({})
    // Similar messages search mode: dismissable chip subject; empty when inactive.
    property string similarSubject: ""
    property var similarTarget: ({})
    // Unsent mail counts for the current account (`mailcore::outbox::status`:
    // `{queued, sending, failed, retryable, pending}`); `{}` before first read.
    property var outboxStatus: ({})

    // --- feed plumbing ----------------------------------------------------

    function reloadFolders() {
        // Synced in place, never cleared: clearing destroys every sidebar
        // delegate on every click (see qml/ModelSync.qml).
        ModelSync.sync(folderModel, FeedJson.parse(backend.folders_json, []), "name");
        // The sidebar shows a subscribed-only subset, and in-place row edits
        // don't re-fire `onFoldersChanged` — refresh the subset explicitly.
        sidebar.refreshShown();
        // Keep selection if still present, else inbox, else first.
        var found = false;
        for (var j = 0; j < folderModel.count; j++) {
            if (folderModel.get(j).name === root.currentFolder) {
                found = true;
                break;
            }
        }
        if (!found) {
            var inbox = "";
            for (var k = 0; k < folderModel.count; k++) {
                if (folderModel.get(k).role === "inbox")
                    inbox = folderModel.get(k).name;
            }
            root.currentFolder = inbox !== "" ? inbox : (folderModel.count > 0 ? folderModel.get(0).name : "");
        }
    }

    function reloadMessages() {
        root.messageRows = FeedJson.parse(backend.messages_json, []);
        // Drop the selection only if that message really is gone.
        if (root.messageByUid(root.currentUid) === undefined)
            root.currentUid = -1;
        if (root.currentUid >= 0)
            root.currentMessage = FeedJson.parse(backend.message_json(root.currentUid), undefined);
        else
            root.currentMessage = undefined;
    }

    function reloadAccounts() {
        ModelSync.sync(accountModel, FeedJson.parse(backend.accounts_json, []), "id");
    }

    function reloadAll() {
        var r = backend.refresh_accounts();
        reloadAccounts();
        reloadFolders();
        reloadMessages();
        reloadOutboxStatus();
        return r;
    }

    // Cheap local read for the footer pill; never touches the list feeds,
    // so read-only jobs can refresh it without losing scroll position.
    function reloadOutboxStatus() {
        root.outboxStatus = FeedJson.parse(backend.outbox_status_json(), ({}));
    }

    function messageByUid(uid) {
        if (uid < 0)
            return undefined;
        for (var i = 0; i < root.messageRows.length; i++) {
            if (root.messageRows[i].uid === uid)
                return root.messageRows[i];
        }
        return undefined;
    }

    function showResult(okMessage, result) {
        root.statusText = result === "" ? okMessage : result;
    }

    function copyToClipboard(text) {
        if (!text)
            return;
        clipboardHelper.text = text;
        clipboardHelper.selectAll();
        clipboardHelper.copy();
        clipboardHelper.clear();
    }

    TextEdit {
        id: clipboardHelper
        visible: false
        width: 0
        height: 0
    }

    function findSimilar(folderPath, uid) {
        var folder = folderPath !== "" ? folderPath : root.currentFolder;
        var subj = backend.find_similar_subject(folder, uid);
        var hits = FeedJson.parse(backend.find_similar_json(folder, uid), []);
        root.similarSubject = subj;
        root.similarTarget = ({
                                  folder: folder,
                                  uid: uid
                              });
        root.searching = true;
        root.searchRows = hits;
        root.searchPlan = ({
                               mode: "similar",
                               query: "",
                               hit_limit: hits.length,
                               debounce_ms: 0
                           });
        if (searchField.text !== "")
            searchField.text = "";
        if (!root.wideLayout) {
            if (root.currentUid >= 0)
                root.closeReader();
            root.narrowPane = "list";
        }
    }

    function clearSimilar() {
        if (root.similarSubject === "")
            return;
        root.similarSubject = "";
        root.similarTarget = ({});
        root.searching = false;
        root.searchRows = [];
        root.lastServerQuery = "";
    }

    // Toolbar search: short input filters the loaded folder feed (see
    // MessageList.matches); 3+ letters run the FTS index — account-wide,
    // or limited to the selected folder while the toolbar checkbox is on
    // (see searchScope). Local SQLite read, cheap enough per keystroke.
    // `fromTyping` marks a real keystroke: only those re-arm the
    // server-search debounce — a job-finish refresh must not, or the
    // finished job would retrigger itself forever.
    function updateSearch(fromTyping) {
        if (root.similarSubject !== "") {
            if (fromTyping) {
                root.clearSimilar();
            } else {
                root.searchRows = FeedJson.parse(backend.find_similar_json(root.similarTarget.folder || "",
                                                                           root.similarTarget.uid), []);
                return;
            }
        }
        root.searchPlan = FeedJson.parse(backend.search_plan_json(searchField.text), ({}));
        if (root.searchPlan.mode === "index") {
            // Starting a search shows the hits: where the list shares its
            // place with the folders or the reader, bring it forward.
            if (!root.searching && !root.wideLayout) {
                if (root.currentUid >= 0)
                    root.closeReader();
                root.narrowPane = "list";
            }
            root.searching = true;
            root.searchRows = FeedJson.parse(backend.search_json(root.searchPlan.query, root.searchScope()), []);
            // Thin local hits get topped up from the server once typing
            // settles (debounced below); the job refresh re-runs this.
            if (fromTyping)
                serverSearchTimer.restart();
        } else {
            root.searching = false;
            root.searchRows = [];
            root.lastServerQuery = "";
            serverSearchTimer.stop();
        }
    }

    // Folder scope for search: the toolbar checkbox limits the local FTS
    // index and the server backfill to the selected folder ("" means the
    // whole account — also while no folder is selected yet).
    function searchScope() {
        if (folderScopeCheck.checked && root.currentFolder !== "")
            return root.currentFolder;
        return "";
    }

    // Ask the server too when the local index runs thin (full local pages
    // need no backfill). Retries while busy; the in-flight flag plus the
    // same-query guard stop overlapping or repeated jobs.
    function kickServerSearch() {
        if (!root.searching || root.serverSearching)
            return;
        var q = root.searchPlan.query;
        if (root.searchPlan.mode !== "index" || q === root.lastServerQuery || root.searchRows.length
                >= root.searchPlan.hit_limit)

            return;
        var r = backend.search_server(q, root.searchScope());
        if (r === "") {
            root.lastServerQuery = q;
            root.serverSearching = true;
            root.statusText = qsTr("Searching server…");
        } else {
            serverSearchTimer.restart();
        }
    }

    // Open a search hit: switch to its folder underneath and open it. The
    // search stays, so Back from the reader returns to the results.
    function jumpToSearchResult(path, uid) {
        root.useSearchFolder(path);
        if (root.currentFolder === path)
            root.openMessage(uid);
    }

    // Bridge actions act on the selected folder: a search hit's row action
    // selects the hit's folder first (the results stay on screen). That
    // switch starts no folder sync: its job would leave the bridge busy, and
    // the action that follows (a permanent delete is a job too) would be
    // refused.
    function useSearchFolder(path) {
        if (path !== "" && root.currentFolder !== path)
            root.selectFolder(path, true);
    }

    // --- actions ----------------------------------------------------------

    function openMessage(uid) {
        if (uid < 0 || uid === root.currentUid)
            // already open: re-clicking a row must not reload anything
            return;
        for (var i = 0; i < folderModel.count; i++) {
            if (folderModel.get(i).name === root.currentFolder && folderModel.get(i).role === "drafts") {
                root.statusText = qsTr("Opening draft…");
                var r = backend.draft_form(uid);
                if (r !== "")
                    root.statusText = r;
                return;
            }
        }
        root.currentUid = uid;
        root.currentMessage = FeedJson.parse(backend.message_json(uid), undefined);
        // Narrow layouts navigate to the reader; wide ones show it already.
        // Drafts open in the composer instead (above), and a message that
        // failed to load stays on the list rather than in an empty reader.
        if (root.currentMessage !== undefined)
            root.narrowPane = "reader";
        markReadTimer.stop();
        // Whether and when viewing marks the row read is the core's call
        // (`store::settings::mark_read_plan`): already-read rows stay
        // untouched instead of issuing a no-op flag write.
        var plan = FeedJson.parse(backend.mark_read_plan_json(root.currentMessage !== undefined
                                                              && root.currentMessage.unread === true), {
                                      plan: "off",
                                      delay_secs: 0
                                  });
        if (plan.plan === "now") {
            markAsRead(uid);
        } else if (plan.plan === "after") {
            // Thunderbird-style: counts as read only if still viewing it
            // when the delay elapses; moving on keeps it unread.
            markReadTimer.uid = uid;
            markReadTimer.interval = plan.delay_secs * 1000;
            markReadTimer.start();
        }
    }

    function markAsRead(uid) {
        if (uid < 0)
            return;
        var r = backend.open_message(uid);
        if (r !== "")
            root.statusText = r;
        // messageRows are plain JS objects: mutating row.unread in place
        // never re-renders the delegate. Rebuild the feed like every
        // other mutation path does instead.
        reloadMessages();
        reloadFolders();
    }

    function syncNow() {
        if (backend.account_count === 0) {
            root.statusText = qsTr("Add an account first");
            return;
        }
        if (root.busy)
            return;
        root.statusText = qsTr("Syncing…");
        var r = backend.sync_now();
        if (r !== "")
            root.statusText = r;
    }

    function loadOlder() {
        if (root.busy)
            return;
        root.statusText = qsTr("Loading older messages…");
        var r = backend.load_older_messages();
        if (r !== "")
            root.statusText = r;
    }

    // The search scope changed (toolbar checkbox or its menu twin): re-run an
    // active search under the new scope, server top-up included.
    function folderScopeToggled() {
        root.lastServerQuery = "";
        root.updateSearch(true);
    }

    function toggleStar(uid) {
        if (uid < 0)
            return;
        showResult("", backend.toggle_star(uid));
        reloadMessages();
        if (root.searching)
            root.updateSearch(false);
    }

    // Moves to Trash — except spam (destroyed outright, junk never touches
    // Trash) and Trash itself (deleting there is permanent). The bridge
    // reports which it did because "moved" and "destroyed" differ.
    // Whether it destroys and whether to ask first is one core rule
    // (`mailcore::undo::delete_prompt`); QML only looks up each target
    // folder's feed flag `delete_is_permanent` (null when not in the feed).
    function folderDeletePermanent(path) {
        var folder = path !== undefined ? path : root.currentFolder;
        for (var i = 0; i < folderModel.count; i++) {
            if (folderModel.get(i).name === folder)
                return folderModel.get(i).delete_is_permanent === true;
        }
        return null;
    }

    // Search selections can span folders: one flag per target.
    function targetsDeletePermanent(targets) {
        if (root.isSearchTargets(targets))
            return targets.map(t => root.folderDeletePermanent(t.folder));
        return [root.folderDeletePermanent()];
    }

    function deletePrompt(bulk, permanent) {
        var json = backend.delete_prompt_json(appSettings.confirm_delete, bulk, JSON.stringify(permanent));
        return FeedJson.parse(json, ({
                "permanent": true,
                "ask": true
            }));
    }

    function deleteMessage(uid) {
        if (uid < 0)
            return;
        var prompt = root.deletePrompt(false, [root.folderDeletePermanent()]);
        if (!prompt.ask) {
            root.doDelete(uid);
            return;
        }
        var m = root.messageByUid(uid);
        deleteConfirm.uid = uid;
        deleteConfirm.uids = [];
        deleteConfirm.subject = m !== undefined ? m.subject : "";
        deleteConfirm.permanent = prompt.permanent;
        deleteConfirm.open();
    }

    function doDelete(uid) {
        if (uid < 0)
            return;
        if (root.currentUid === uid) {
            root.currentUid = -1;
            root.currentMessage = undefined;
        }
        root.statusText = qsTr("Deleting…");
        var r = backend.delete_message(uid);
        if (r !== "")
            root.statusText = r;
    }

    function archiveMessage(uid) {
        if (uid < 0)
            return;
        if (root.currentUid === uid)
            root.currentUid = -1;
        root.statusText = qsTr("Archiving…");
        var r = backend.archive_message(uid);
        if (r !== "")
            root.statusText = r;
    }

    // Move picker: remembers which message, the dialog reports the target.
    function openMove(uid) {
        if (uid < 0)
            return;
        var m = root.messageByUid(uid);
        moveDialog.uid = uid;
        moveDialog.uids = [];
        moveDialog.subject = m !== undefined ? m.subject : "";
        moveDialog.open();
    }

    // Bulk move picker: remembers the whole checkbox set.
    function openBulkMove(uids) {
        if (!uids || uids.length === 0)
            return;
        moveDialog.uid = -1;
        moveDialog.uids = uids.slice();
        moveDialog.subject = "";
        moveDialog.open();
    }

    function purgeMessage(uid) {
        if (uid < 0)
            return;
        if (root.currentUid === uid)
            root.currentUid = -1;
        root.statusText = qsTr("Deleting…");
        var r = backend.purge_message(uid);
        if (r !== "")
            root.statusText = r;
    }

    function confirmPurge(uid) {
        if (uid < 0)
            return;
        var m = root.messageByUid(uid);
        purgeConfirm.uid = uid;
        purgeConfirm.uids = [];
        purgeConfirm.subject = m !== undefined ? m.subject : "";
        purgeConfirm.open();
    }

    function confirmBulkPurge(uids) {
        if (!uids || uids.length === 0)
            return;
        purgeConfirm.uid = -1;
        purgeConfirm.uids = uids.slice();
        purgeConfirm.subject = "";
        purgeConfirm.open();
    }

    // --- bulk selection actions (Roundcube-style, one backend call) --------

    // Bulk targets are plain UIDs from a folder list, or {folder, uid} from
    // search results. Search targets go to the bridge's `*_hits` calls in one
    // piece: mailcore groups them by folder and answers with one Undo and
    // one purge job (`mailcore::bulk`), so no folder is switched underneath.
    function isSearchTargets(targets) {
        return targets.length > 0 && typeof targets[0] === "object";
    }

    // The reader's message is among `targets` (search targets count only in
    // the folder it is open in).
    function dropPreviewIfGone(targets) {
        if (root.currentUid < 0)
            return;
        for (var i = 0; i < targets.length; i++) {
            var t = targets[i];
            var hit = typeof t === "object" ? (t.folder === root.currentFolder && t.uid === root.currentUid) : t
                                              === root.currentUid;
            if (hit) {
                root.currentUid = -1;
                return;
            }
        }
    }

    // One bridge call for the whole selection: `hits` for search targets,
    // `many` for UIDs of the shown folder. Search results re-read after.
    function runBulk(targets, many, hits) {
        if (!targets || targets.length === 0)
            return "";
        var json = JSON.stringify(targets);
        var r = root.isSearchTargets(targets) ? hits(json) : many(json);
        if (root.searching)
            root.updateSearch(false);
        return r;
    }

    function bulkMarkRead(targets, read) {
        var r = root.runBulk(targets, json => backend.mark_read_many(json, read), json => backend.mark_read_hits(json,
                                                                                                                 read));
        reloadFolders();
        reloadMessages();
        root.statusText = r;
    }

    function bulkStar(targets, starred) {
        var r = root.runBulk(targets, json => backend.set_star_many(json, starred), json => backend.set_star_hits(json,
                                                                                                                  starred));
        reloadMessages();
        root.statusText = r;
    }

    function bulkArchive(targets) {
        root.dropPreviewIfGone(targets);
        root.statusText = qsTr("Archiving…");
        var r = root.runBulk(targets, json => backend.archive_many(json), json => backend.archive_hits(json));
        if (r !== "")
            root.statusText = r;
    }

    function bulkMoveTo(targets, path) {
        root.dropPreviewIfGone(targets);
        root.statusText = qsTr("Moving…");
        var r = root.runBulk(targets, json => backend.move_many(json, path), json => backend.move_hits(json, path));
        if (r !== "")
            root.statusText = r;
    }

    function bulkDelete(targets) {
        if (!targets || targets.length === 0)
            return;
        var prompt = root.deletePrompt(true, root.targetsDeletePermanent(targets));
        if (!prompt.ask) {
            root.doBulkDelete(targets);
            return;
        }
        deleteConfirm.uid = -1;
        deleteConfirm.uids = targets.slice();
        deleteConfirm.subject = "";
        deleteConfirm.permanent = prompt.permanent;
        deleteConfirm.open();
    }

    function doBulkDelete(targets) {
        root.dropPreviewIfGone(targets);
        root.statusText = qsTr("Deleting…");
        var r = root.runBulk(targets, json => backend.delete_many(json), json => backend.delete_hits(json));
        if (r !== "")
            root.statusText = r;
    }

    function bulkPurge(targets) {
        root.dropPreviewIfGone(targets);
        root.statusText = qsTr("Deleting…");
        var r = root.runBulk(targets, json => backend.purge_many(json), json => backend.purge_hits(json));
        if (r !== "")
            root.statusText = r;
    }

    function changeSort(field, descending) {
        var r = backend.set_sort(field, descending);
        if (r !== "") {
            root.statusText = r;
            return;
        }
        reloadMessages();
        var label = field === "from" ? qsTr("From") : field === "subject" ? qsTr("Subject") : qsTr("Date");
        var dir = descending ? qsTr("descending") : qsTr("ascending");
        root.statusText = qsTr("Sorted by %1 (%2)").arg(label).arg(dir);
    }

    // `underSearch`: switched underneath the search results (see
    // useSearchFolder) — the results, their checkboxes and the search stay
    // as they are, and no server sync starts.
    function selectFolder(path, underSearch) {
        var r = backend.select_folder(path);
        if (r === "") {
            markReadTimer.stop();
            if (underSearch !== true)
                messageList.setSelectionMode(false);
            root.currentFolder = path;
            root.currentUid = -1;
            // Narrow layouts return to the list; wide ones show it already.
            root.narrowPane = "list";
            reloadMessages();
            if (underSearch === true)
                return;
            // A folder-scoped search follows the selection: fresh scope,
            // fresh server top-up for the newly shown folder.
            if (folderScopeCheck.checked)
                root.lastServerQuery = "";
            root.updateSearch(true);
            // A folder click must only read the local cache. `sync_folder_now`
            // SELECTs, SEARCHes every server UID and can download a 200-mail
            // window; doing that synchronously here freezes Qt long enough for
            // the desktop's "not responding" watchdog. Startup, auto-check
            // and the toolbar Sync button refresh the server separately.
            root.statusText = qsTr("Folder: %1").arg(path);
            if (!root.busy)
                backend.sync_folder_now(path);
        } else {
            root.statusText = r;
        }
    }

    // Jump request from the bar widget / a notification (`mailapp --open`):
    // takes "<account_id>\n<folder>" (empty folder = inbox), switches the
    // account, lands on the folder and raises the window. Take-once on the
    // Rust side — each click jumps exactly once. Returns true when a jump
    // happened.
    function consumePendingOpen() {
        var r = backend.consume_pending_open();
        if (r === "")
            return false;
        var nl = r.indexOf("\n");
        var id = parseInt(nl < 0 ? r : r.slice(0, nl), 10);
        var folder = nl < 0 ? "" : r.slice(nl + 1);
        if (!isFinite(id) || id <= 0)
            return false;
        if (id !== backend.current_account_id)
            selectAccount(id);
        // Let the account switch settle (feeds reload) before landing.
        var target = folder;
        Qt.callLater(function () {
            if (backend.current_account_id !== id)
                return;
            if (target !== "" && target !== root.currentFolder)
                selectFolder(target);
            // The click came from outside: raise above the bar popup.
            root.raise();
            root.requestActivate();
        });
        return true;
    }

    function selectAccount(id) {
        var r = backend.select_account(id);
        if (r === "") {
            markReadTimer.stop();
            messageList.setSelectionMode(false);
            root.currentUid = -1;
            root.currentFolder = "";
            reloadAccounts();
            reloadFolders();
            reloadMessages();
            // An active search belongs to the previous account: re-run it
            // here so results (and any server top-up) follow the switch.
            root.updateSearch(true);
            root.statusText = qsTr("Account: %1").arg(backend.current_account_email);
            // Render the selected account's cache before the synchronous
            // account-scoped refresh begins. `sync_now` only ever uses
            // `current_account_id`, so inactive accounts are never loaded.
            Qt.callLater(function () {
                if (!root.busy && backend.current_account_id === id)
                    root.syncNow();
            });
        } else {
            root.statusText = r;
        }
    }

    // Windows draws the caption bar outside the Qt scene and does not follow
    // the desktop colour scheme on its own, so a dark app came up with a white
    // title bar. The bridge tells DWM; on Linux it does nothing. Driven from a
    // timer because the native window has to exist first, and re-applied if
    // the desktop switches between light and dark while running.
    Timer {
        interval: 0
        running: true
        repeat: false
        onTriggered: backend.apply_native_theme(Theme.dark)
    }

    Connections {
        target: Application.styleHints
        function onColorSchemeChanged() {
            backend.apply_native_theme(Theme.dark);
        }
    }

    // The bridge exposes its signal under the Rust name, so the handler is
    // `onJob_finished` (Qt only capitalises the first letter) — `onJobFinished`
    // silently matches nothing and the whole block never runs.
    Connections {
        id: jobConnections
        target: backend
        // Jobs that only read: nothing they did is visible in the feeds, so
        // reloading would throw away the list's scroll position for free.
        readonly property var readOnlyKinds: ["Open", "Open draft", "Save", "Capabilities"]

        // SMTP has accepted the message; the Sent copy and the folder
        // resync still have to run, but the user is done waiting.
        // Only while this send is still pending: once released, the composer
        // may already hold a newer message the user started.
        function onJob_progress(kind, status) {
            if (kind === "Send") {
                if (composer.sendPending) {
                    composer.sendPending = false;
                    composer.markClean();
                }
                root.statusText = qsTr("Sent");
            } else if (kind === "Sync" && status !== "") {
                // Per-folder progress from the sync job ("Syncing 3/15: …").
                root.statusText = status;
            }
        }

        // A delete/archive/move was queued: the rows are already gone from
        // the bridge's feeds, and the toast offers Undo until the push.
        function onUndo_available(batch, label) {
            reloadFolders();
            reloadMessages();
            if (root.searching)
                root.updateSearch(false);
            undoToast.show(batch, label);
        }

        function onJob_finished(kind, status, outcome) {
            if (jobConnections.readOnlyKinds.indexOf(kind) < 0) {
                reloadAccounts();
                reloadFolders();
                reloadMessages();
            }
            reloadOutboxStatus();
            // A sync can change what the index holds: re-run an active
            // search so results never go stale behind a fresh feed (local
            // re-query only — never re-arms the server debounce).
            if (root.searching)
                root.updateSearch(false);
            if (kind === "Search")
                root.serverSearching = false;
            if (kind === "Capabilities") {
                // Owned by the Settings About pane (its own Connections parses
                // the JSON payload); keep it off the status bar.
                return;
            }
            if (kind === "Send") {
                // Closed when queued. `sendPending` still set means progress
                // never arrived, so the composer still holds this message
                // (new compositions are refused meanwhile). Cleared means it
                // was released at SMTP acceptance and may hold a newer mail
                // by now: never touch it then. "sent_partial" means the
                // bookkeeping failed, never the delivery.
                if (composer.sendPending) {
                    composer.sendPending = false;
                    if (outcome === "sent" || outcome === "sent_partial") {
                        composer.markClean();
                        composer.close();
                    } else {
                        // Genuine send failure after the optimistic close:
                        // the fields still hold the text, so reopen and flag
                        // dirty — cancelling then asks before discarding.
                        composer.dirty = true;
                        composer.open();
                    }
                }
                if (status !== "")
                    root.statusText = status;
                return;
            }
            if (kind === "Save draft") {
                // Editing was locked for the save, so the fields still hold
                // exactly what was saved. A partial save already appended the
                // replacement, so reopening and retrying would duplicate it.
                composer.saving = false;
                if (status === "" || status.indexOf("draft saved, but") === 0) {
                    composer.markClean();
                    composer.close();
                    root.statusText = status === "" ? qsTr("Draft saved") : status;
                } else {
                    root.statusText = status;
                }
                return;
            }
            if (kind === "Open") {
                if (status.indexOf("file://") === 0)
                    Qt.openUrlExternally(status);
                else
                    root.statusText = status;
                return;
            }
            if (kind === "Open draft") {
                try {
                    var draft = JSON.parse(status);
                    if (draft.draft_uid === undefined)
                        root.statusText = qsTr("Draft is no longer available");
                    else
                        composer.openForDraft(draft);
                } catch (e) {
                    root.statusText = status === "" ? qsTr("Draft is no longer available") : status;
                }
                return;
            }
            root.statusText = status;
        }
    }

    // Delayed mark-as-read: fires only while the same message is still open.
    Timer {
        id: markReadTimer
        property int uid: -1
        repeat: false
        onTriggered: {
            if (markReadTimer.uid >= 0 && markReadTimer.uid === root.currentUid)
                root.markAsRead(markReadTimer.uid);
        }
    }

    // Server SEARCH backfill waits for typing to settle so every keystroke
    // does not buy a full multi-folder IMAP round.
    Timer {
        id: serverSearchTimer
        interval: root.searchPlan.debounce_ms || 0
        repeat: false
        onTriggered: root.kickServerSearch()
    }

    // Picks up `mailapp --open` clicks while the window is already up:
    // one cheap settings read every 2s, a queued jump switches account,
    // lands on the inbox and raises the window.
    Timer {
        id: pendingOpenTimer
        interval: 2000
        running: true
        repeat: true
        onTriggered: root.consumePendingOpen()
    }

    // The open account's check interval: its own, or the app-wide default.
    // Re-read on account switch and after Settings saves (the revision).
    property int syncSettingsRevision: 0
    readonly property int autoSyncMinutes: root.syncSettingsRevision >= 0 && backend.current_account_id >= 0
                                           ? appSettings.sync_interval_for(backend.current_account_id) : 0

    // Automatic mail check: only while idle (never mid-action), manual-only
    // when the interval is 0. Bound to the setting, so Save applies it live.
    Timer {
        id: autoSyncTimer
        interval: Math.max(1, root.autoSyncMinutes) * 60000
        running: root.autoSyncMinutes > 0
        repeat: true
        onTriggered: {
            if (root.busy || backend.account_count === 0)
                return;
            // Quiet hours hold the check back while nobody looks at the
            // window; a focused window always checks.
            if (!root.active && root.quietNow(backend.current_account_id))
                return;
            root.syncNow();
        }
    }

    function quietNow(accountId) {
        return accountId >= 0 && FeedJson.parse(appSettings.account_settings_json(accountId), {}).quiet_now === true;
    }

    Component.onCompleted: {
        appSettings.load();
        root.narrowPane = appSettings.start_view === "inbox" ? "list" : "folders";
        var r = reloadAll();
        if (backend.account_count === 0) {
            root.statusText = qsTr("Add an account to start");
            accountSetup.openNew();
        } else if (r !== "") {
            root.statusText = r;
        } else if (root.consumePendingOpen()) {
            // Cold start from a widget/notification click: already on the
            // right account + inbox. A switch syncs by itself; same-account
            // clicks refresh here (the deferred switch sync skips on busy).
            root.narrowPane = "list";
            Qt.callLater(function () {
                if (!root.busy && backend.account_count > 0)
                    root.syncNow();
            });
        } else {
            root.statusText = qsTr("Ready");
            // Refresh on startup: show the cache immediately, then sync.
            // Deferred so first paint happens first (sync blocks on network).
            Qt.callLater(function () {
                if (backend.account_count > 0)
                    root.syncNow();
            });
        }
    }

    // --- keyboard ---------------------------------------------------------

    Shortcut {
        sequences: ["Ctrl+N"]
        onActivated: composer.openBlank()
    }
    Shortcut {
        sequences: ["Ctrl+R", "F5"]
        onActivated: root.syncNow()
    }
    Shortcut {
        sequences: ["Ctrl+F"]
        onActivated: searchField.forceActiveFocus()
    }
    Shortcut {
        sequences: ["Down"]
        onActivated: messageList.step(1)
    }
    Shortcut {
        sequences: ["Up"]
        onActivated: messageList.step(-1)
    }
    Shortcut {
        sequences: ["Delete"]
        onActivated: root.deleteMessage(root.currentUid)
    }
    Shortcut {
        sequences: ["Shift+Delete"]
        onActivated: root.confirmPurge(root.currentUid)
    }
    Shortcut {
        sequences: ["S"]
        onActivated: root.toggleStar(root.currentUid)
    }
    Shortcut {
        sequences: ["A"]
        onActivated: root.archiveMessage(root.currentUid)
    }
    Shortcut {
        sequences: ["M"]
        onActivated: root.openMove(root.currentUid)
    }
    Shortcut {
        sequences: ["R"]
        onActivated: if (root.currentUid >= 0)
                         composer.openForAnswer(root.currentUid, "reply")
    }
    Shortcut {
        sequences: ["F"]
        onActivated: if (root.currentUid >= 0)
                         composer.openForAnswer(root.currentUid, "forward")
    }
    Shortcut {
        sequences: ["F11"]
        onActivated: root.toggleReaderFullscreen()
    }
    Shortcut {
        sequences: ["Esc"]
        onActivated: if (root.readerFullscreen)
                         root.toggleReaderFullscreen()
    }

    // --- chrome -----------------------------------------------------------

    header: Rectangle {
        visible: !root.readerFullscreen
        enabled: !root.readerFullscreen
        implicitHeight: Theme.toolbarHeight
        color: Theme.bgAlt

        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: Theme.border
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: Theme.sm
            anchors.rightMargin: Theme.sm
            spacing: Theme.sm

            IconButton {
                visible: root.wideLayout
                text: Icons.menu
                iconFont: true
                tooltip: qsTr("Toggle sidebar")
                onClicked: root.sidebarOpen = !root.sidebarOpen
            }
            // Narrow layouts have no sidebar to toggle: this steps back to
            // the folder pane instead.
            IconButton {
                visible: !root.wideLayout && !root.mediumLayout && root.narrowPane === "list"
                text: Icons.arrowBack
                iconFont: true
                tooltip: qsTr("Folders")
                onClicked: root.narrowPane = "folders"
            }

            AppButton {
                visible: !root.compactToolbar
                text: qsTr("✎  Compose")
                intent: "primary"
                enabled: backend.account_count > 0
                onClicked: composer.openBlank()
            }
            IconButton {
                visible: root.compactToolbar
                text: Icons.edit
                iconFont: true
                tooltip: qsTr("Compose (Ctrl+N)")
                contentColor: Theme.accent
                enabled: backend.account_count > 0
                onClicked: composer.openBlank()
            }

            // Live filter over the loaded feed (short input); 3+ letters run
            // the FTS index instead (account-wide, or this folder when the
            // checkbox is on — see updateSearch).
            TextField {
                id: searchField
                Layout.fillWidth: true
                Layout.minimumWidth: 60
                Layout.maximumWidth: 460
                implicitHeight: Theme.controlHeight
                placeholderText: root.compactToolbar ? (folderScopeCheck.checked ? qsTr("Search folder…") : qsTr(
                                                                                       "Search…")) :
                                                       folderScopeCheck.checked ? qsTr(
                                                                                      "Search this folder… (3+ letters: folder + server)") :
                                                                                  qsTr("Search mail… (3+ letters: account + server)")
                color: Theme.text
                placeholderTextColor: Theme.textMuted
                font.pixelSize: Theme.fontBase
                onTextChanged: root.updateSearch(true)
                leftPadding: Theme.sm
                rightPadding: clearSearch.visible ? clearSearch.width + Theme.xs : Theme.sm
                selectByMouse: true

                background: Rectangle {
                    radius: Theme.radius
                    color: Theme.bg
                    border.width: 1
                    border.color: searchField.activeFocus ? Theme.accent : Theme.border
                }

                IconButton {
                    id: clearSearch
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    width: Theme.miniButton
                    height: Theme.miniButton
                    visible: searchField.text !== "" || root.similarSubject !== ""
                    text: Icons.close
                    iconFont: true
                    fontSize: Theme.fontSmall
                    tooltip: qsTr("Clear search")
                    onClicked: {
                        searchField.text = "";
                        root.clearSimilar();
                    }
                }
                Keys.onEscapePressed: {
                    searchField.text = "";
                    root.clearSimilar();
                }
                // The search syntax, as mailcore::search reads it.
                ToolTip.text: backend.search_syntax_help()
                ToolTip.visible: hovered
                ToolTip.delay: 800
            }

            // Folder scope: limits the FTS index and the server backfill
            // to the selected folder (off = whole account). Toggling
            // re-runs an active search under the new scope.
            // Compact toolbars hide it; the overflow menu toggles the same
            // state (the checkbox keeps holding it either way).
            CheckBox {
                id: folderScopeCheck
                visible: !root.compactToolbar
                text: root.width > 640 ? qsTr("Folder") : ""
                Accessible.name: qsTr("Search only the current folder")
                ToolTip.text: qsTr("Search only the current folder")
                ToolTip.visible: hovered
                onToggled: root.folderScopeToggled()
            }

            Item {
                Layout.fillWidth: true
            }

            IconButton {
                text: Icons.sync
                iconFont: true
                tooltip: qsTr("Sync now (Ctrl+R)")
                enabled: backend.account_count > 0 && !root.busy
                onClicked: root.syncNow()
            }
            IconButton {
                visible: !root.compactToolbar
                text: Icons.folder
                iconFont: true
                tooltip: qsTr("Manage folders")
                enabled: backend.account_count > 0
                onClicked: foldersDialog.open()
            }
            IconButton {
                visible: !root.compactToolbar
                text: Icons.contacts
                iconFont: true
                tooltip: qsTr("Contacts")
                onClicked: contactsDialog.open()
            }
            IconButton {
                visible: !root.compactToolbar
                text: Icons.person
                iconFont: true
                tooltip: qsTr("Accounts")
                onClicked: accountsDialog.open()
            }
            IconButton {
                visible: !root.compactToolbar
                text: Icons.settings
                iconFont: true
                tooltip: qsTr("Settings")
                onClicked: settingsDialog.open()
            }
            IconButton {
                id: overflowButton
                visible: root.compactToolbar
                text: Icons.moreVert
                iconFont: true
                tooltip: qsTr("More")
                onClicked: overflowMenu.popup(overflowButton, 0, overflowButton.height)
            }
        }

        AppMenu {
            id: overflowMenu
            AppMenuItem {
                glyph: folderScopeCheck.checked ? Icons.checkBox : Icons.checkBoxBlank
                label: qsTr("Search only this folder")
                onTriggered: {
                    folderScopeCheck.checked = !folderScopeCheck.checked;
                    root.folderScopeToggled();
                }
            }
            AppMenuItem {
                glyph: Icons.folder
                label: qsTr("Manage folders")
                enabled: backend.account_count > 0
                onTriggered: foldersDialog.open()
            }
            AppMenuItem {
                glyph: Icons.contacts
                label: qsTr("Contacts")
                onTriggered: contactsDialog.open()
            }
            AppMenuItem {
                glyph: Icons.person
                label: qsTr("Accounts")
                onTriggered: accountsDialog.open()
            }
            AppMenuItem {
                glyph: Icons.settings
                label: qsTr("Settings")
                onTriggered: settingsDialog.open()
            }
        }
    }

    SplitView {
        anchors.fill: parent

        // A grabbable split: the hit area is wide, only the line is thin.
        handle: Item {
            implicitWidth: 9
            Rectangle {
                width: 1
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                anchors.horizontalCenter: parent.horizontalCenter
                color: SplitHandle.pressed || SplitHandle.hovered ? Theme.accent : Theme.border
            }
            HoverHandler {
                cursorShape: Qt.SplitHCursor
            }
        }

        Sidebar {
            id: sidebar
            visible: !root.readerFullscreen && (root.wideLayout ? root.sidebarOpen : root.mediumLayout ? true :
                                                                                                         root.narrowPane
                                                                                                         === "folders")
            enabled: visible
            SplitView.preferredWidth: 250
            SplitView.minimumWidth: 160
            folders: folderModel
            accounts: accountModel
            currentFolder: root.currentFolder
            currentEmail: backend.current_account_email
            currentAccountId: backend.current_account_id
            onFolderSelected: path => root.selectFolder(path)
            onAccountSelected: id => root.selectAccount(id)
        }

        MessageList {
            id: messageList
            visible: !root.readerFullscreen && (root.wideLayout ? true : root.mediumLayout ? root.currentUid < 0 : root.narrowPane
                                                                                             === "list")
            enabled: visible
            SplitView.preferredWidth: 360
            SplitView.minimumWidth: 240
            messages: root.messageRows
            currentUid: root.currentUid
            folderName: root.currentFolder
            filterText: searchField.text
            backend: backend
            totalCount: backend.messages_total
            serverTotal: backend.messages_server_total
            olderState: backend.messages_older
            olderCanLoad: backend.messages_can_load_older
            limit: backend.message_limit
            busy: root.busy
            sortField: backend.sort_field
            sortDescending: backend.sort_descending
            density: appSettings.list_density
            searching: root.searching
            deletePermanentFor: targets => root.deletePrompt(true, root.targetsDeletePermanent(targets)).permanent
            searchRows: root.searchRows
            // Similar results span the account whatever the scope checkbox says.
            searchFolder: root.searching && root.similarSubject === "" ? root.searchScope() : ""
            similarSubject: root.similarSubject
            serverSearching: root.serverSearching
            onMessageSelected: uid => root.openMessage(uid)
            onSearchJump: (path, uid) => root.jumpToSearchResult(path, uid)
            onSearchFolderNeeded: path => root.useSearchFolder(path)
            onFindSimilarRequested: (folderPath, uid) => root.findSimilar(folderPath, uid)
            onClearSimilarRequested: root.clearSimilar()
            onStarToggled: uid => root.toggleStar(uid)
            onArchiveRequested: uid => root.archiveMessage(uid)
            onMoveRequested: uid => root.openMove(uid)
            onMarkReadRequested: (uid, read) => {
                var r = backend.mark_read(uid, read);
                reloadFolders();
                reloadMessages();
                if (root.searching)
                    root.updateSearch(false);
                root.statusText = r !== "" ? r : (read ? qsTr("Marked as read") : qsTr("Marked as unread"));
            }
            onDeleteRequested: uid => root.deleteMessage(uid)
            onPurgeRequested: uid => root.confirmPurge(uid)
            onBulkMarkReadRequested: (uids, read) => root.bulkMarkRead(uids, read)
            onBulkStarRequested: (uids, starred) => root.bulkStar(uids, starred)
            onBulkArchiveRequested: uids => root.bulkArchive(uids)
            onBulkMoveRequested: uids => root.openBulkMove(uids)
            onBulkDeleteRequested: uids => root.bulkDelete(uids)
            onBulkPurgeRequested: uids => root.confirmBulkPurge(uids)
            onLoadOlderRequested: root.loadOlder()
            onSortRequested: (field, descending) => root.changeSort(field, descending)
            onStatusMessage: text => root.statusText = text
        }

        MessageView {
            id: messageView
            SplitView.fillWidth: true
            SplitView.minimumWidth: 260
            visible: root.wideLayout ? true : root.mediumLayout ? root.currentUid >= 0 : root.narrowPane === "reader"
            enabled: visible
            isFullscreen: root.readerFullscreen
            showBack: root.mediumLayout ? root.currentUid >= 0 : !root.wideLayout && root.narrowPane === "reader"
            onBackRequested: root.closeReader()
            loadRemoteImages: appSettings.load_remote_images
            readerFont: appSettings.reader_font_size
            linkClickAction: appSettings.link_click_action
            backend: backend
            message: root.currentMessage
            onReplyRequested: composer.openForAnswer(root.currentUid, "reply")
            onReplyAllRequested: composer.openForAnswer(root.currentUid, "reply_all")
            onForwardRequested: composer.openForAnswer(root.currentUid, "forward")
            onFindSimilarRequested: root.findSimilar("", root.currentUid)
            onStarRequested: root.toggleStar(root.currentUid)
            onArchiveRequested: root.archiveMessage(root.currentUid)
            onMoveRequested: root.openMove(root.currentUid)
            onDeleteRequested: root.deleteMessage(root.currentUid)
            onPurgeRequested: root.confirmPurge(root.currentUid)
            onFullscreenRequested: root.toggleReaderFullscreen()
            onStatusMessage: text => root.statusText = text
        }
    }

    // A bulk action over search results undoes as one: its batches arrive
    // joined (uuids, so the comma cannot clash).
    function undoMove(batch) {
        var parts = batch.split(",");
        for (var i = 0; i < parts.length; i++)
            root.statusText = backend.undo_move(parts[i]);
        reloadFolders();
        reloadMessages();
        if (root.searching)
            root.updateSearch(false);
    }

    // Floats above the status bar; hidden until an undoable action runs.
    UndoToast {
        id: undoToast
        z: 100
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Theme.md
        durationMs: backend.undo_grace_secs() * 1000
        onUndoRequested: batch => root.undoMove(batch)
    }

    Shortcut {
        sequences: [StandardKey.Undo]
        enabled: undoToast.visible
        onActivated: undoToast.undo()
    }

    footer: Rectangle {
        implicitHeight: 26
        color: Theme.bgAlt

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
                text: root.busy ? Icons.sync : ""
                font.family: Icons.fontFamily
                color: Theme.accent
                font.pixelSize: Theme.fontSmall
            }
            // Read-only TextEdit instead of a Label so the status line can be
            // selected and copied (drag + Ctrl+C); styled to look identical.
            // Hovering displays the full text in a ToolTip so larger entries
            // are not cut off. Double-clicking or clicking the details button
            // opens a dialog to view and copy the complete message.
            TextEdit {
                id: statusTextEdit
                Layout.fillWidth: true
                text: root.statusText
                color: (root.statusText.toLowerCase().indexOf("error") >= 0 || root.statusText.toLowerCase().indexOf(
                            "failed") >= 0) ? Theme.danger : Theme.textMuted
                font.pixelSize: Theme.fontSmall
                readOnly: true
                selectByMouse: true
                wrapMode: Text.NoWrap
                clip: true

                HoverHandler {
                    id: statusHoverHandler
                }

                ToolTip.visible: statusHoverHandler.hovered && root.statusText.length > 0
                ToolTip.text: root.statusText
                ToolTip.delay: 400

                TapHandler {
                    acceptedButtons: Qt.RightButton
                    onTapped: statusContextMenu.popup()
                }

                TapHandler {
                    onDoubleTapped: statusDetailsDialog.open()
                }
            }

            IconButton {
                id: statusDetailsButton
                visible: root.statusText !== ""
                implicitWidth: Theme.miniButton
                implicitHeight: Theme.miniButton
                fontSize: Theme.fontSmall
                text: Icons.openInNew
                iconFont: true
                tooltip: qsTr("View full status message and copy")
                onClicked: statusDetailsDialog.open()
            }
            // Unsent mail: queued, still sending, or failed. Opens the
            // outbox dialog; red while anything failed.
            IconButton {
                id: outboxButton
                visible: (root.outboxStatus.pending || 0) > 0
                implicitWidth: Theme.miniButton
                implicitHeight: Theme.miniButton
                fontSize: Theme.fontSmall
                text: Icons.outbox
                iconFont: true
                tooltip: qsTr("Outbox: %1 — open outbox").arg(root.outboxStatus.label || "")
                Accessible.name: tooltip
                onClicked: outboxDialog.open()
            }
            Label {
                visible: outboxButton.visible
                text: root.outboxStatus.pending || 0
                color: root.outboxStatus.has_failures ? Theme.danger : Theme.textMuted
                font.pixelSize: Theme.fontSmall
            }
            Label {
                text: backend.current_account_email
                color: Theme.textMuted
                font.pixelSize: Theme.fontTiny
                elide: Text.ElideRight
            }
        }
    }

    // --- dialogs ----------------------------------------------------------

    Composer {
        id: composer
        accountEmail: backend.current_account_email
        accountFromName: backend.current_account_from_name
        sendFormat: appSettings.compose_send_format
        backend: backend
        collectContacts: appSettings.collect_sent_contacts
        onStatusMessage: text => root.statusText = text
        onSendRequested: payload => {
            root.statusText = qsTr("Sending…");
            var r = backend.send_mail(payload);
            if (r !== "") {
                root.statusText = r;
            } else {
                // Validated + queued locally (no network yet): close at once
                // instead of waiting out the SMTP transaction. A later
                // failure reopens the composer with the text still in place.
                composer.sendPending = true;
                composer.markClean();
                composer.close();
            }
        }
        onSaveDraftRequested: payload => {
            root.statusText = qsTr("Saving draft…");
            // Stays open until the job reports back: closing on the queue
            // acknowledgement would discard the text if the save then failed.
            var r = backend.save_draft(payload);
            if (r !== "")
                root.statusText = r;
            else
                composer.saving = true;
        }
    }

    AccountSetup {
        id: accountSetup
        backend: backend
        onStatusMessage: text => root.statusText = text
        onAccountSubmit: payload => {
            var r = backend.add_account(payload);
            if (r === "") {
                var wasEditing = accountSetup.editing;
                accountSetup.close();
                reloadAll();
                root.statusText = wasEditing ? qsTr("Account updated") : qsTr("Account added — press ⟳ to sync");
            } else {
                root.statusText = r;
            }
        }
    }

    Accounts {
        id: accountsDialog
        accounts: accountModel
        currentAccountId: backend.current_account_id
        onStatusMessage: text => root.statusText = text
        onAddRequested: accountSetup.openNew()
        onEditRequested: id => accountSetup.openEdit(backend.account_form(id), id)
        onAccountSelected: id => root.selectAccount(id)
        onDeleteConfirmed: id => {
            var r = backend.delete_account(id);
            reloadAll();
            showResult(qsTr("Account removed"), r);
        }
    }

    Contacts {
        id: contactsDialog
        backend: backend
        onStatusMessage: text => root.statusText = text
    }

    Outbox {
        id: outboxDialog
        backend: backend
        onStatusMessage: text => root.statusText = text
        onSyncRequested: root.syncNow()
        onClosed: root.reloadOutboxStatus()
    }

    Folders {
        id: foldersDialog
        folders: folderModel
        currentFolder: root.currentFolder
        busy: root.busy
        onStatusMessage: text => root.statusText = text
        onRefreshRequested: {
            if (root.busy)
                return;
            root.statusText = qsTr("Refreshing folders…");
            var r = backend.refresh_folders();
            if (r !== "")
                root.statusText = r;
        }
        onVisibilityToggled: (path, subscribed) => {
            showResult("", backend.set_folder_subscribed(path, subscribed));
            reloadFolders();
        }
        onCreateRequested: path => {
            foldersDialog.clearNewFolder();
            root.statusText = qsTr("Creating folder…");
            var r = backend.create_folder(path);
            if (r !== "")
                root.statusText = r;
        }
        onFolderSelected: path => {
            foldersDialog.close();
            // Out of the click handler: selecting rebuilds the feed.
            Qt.callLater(root.selectFolder, path);
        }
    }

    MoveTo {
        id: moveDialog
        folders: folderModel
        currentFolder: root.currentFolder
        onFolderChosen: path => {
            var targets = moveDialog.uids && moveDialog.uids.length > 0 ? moveDialog.uids.slice() : [moveDialog.uid];
            moveDialog.close();
            // Out of the click handler: moving rebuilds the feed.
            Qt.callLater(function () {
                var r;
                root.statusText = qsTr("Moving…");
                if (targets.length > 1 || (moveDialog.uids && moveDialog.uids.length > 0)) {
                    root.bulkMoveTo(targets, path);
                    return;
                } else {
                    var target = targets[0];
                    if (root.currentUid === target)
                        root.currentUid = -1;
                    r = backend.move_message(target, path);
                }
                if (r !== "")
                    root.statusText = r;
            });
        }
    }

    // Trash is reversible (unlike purge), so the move variant uses the
    // primary intent — the danger styling stays reserved for permanent
    // destruction (Trash/Junk source, or no Trash folder at all).
    Dialog {
        id: deleteConfirm
        title: deleteConfirm.permanent ? qsTr("Delete permanently?") : qsTr("Move to Trash?")
        modal: true
        anchors.centerIn: parent
        // Clamped to the window, which may be narrower than the default.
        width: Math.min(420, root.width - 16)
        padding: Theme.lg

        property int uid: -1
        property var uids: []
        property string subject: ""
        property bool permanent: false

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        footer: RowLayout {
            spacing: Theme.sm
            Item {
                Layout.fillWidth: true
            }
            AppButton {
                text: qsTr("Cancel")
                onClicked: deleteConfirm.close()
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                Layout.topMargin: Theme.sm
                text: deleteConfirm.permanent ? qsTr("Delete permanently") : qsTr("Move to Trash")
                intent: deleteConfirm.permanent ? "danger" : "primary"
                onClicked: {
                    var targets = deleteConfirm.uids && deleteConfirm.uids.length > 0 ? deleteConfirm.uids.slice() :
                                                                                        [deleteConfirm.uid];
                    var bulk = deleteConfirm.uids && deleteConfirm.uids.length > 0;
                    deleteConfirm.close();
                    // Out of the click handler: deleting rebuilds the feed.
                    Qt.callLater(function () {
                        if (bulk)
                            root.doBulkDelete(targets);
                        else
                            root.doDelete(targets[0]);
                    });
                }
            }
        }

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            color: Theme.text
            font.pixelSize: Theme.fontBase
            text: deleteConfirm.uids && deleteConfirm.uids.length > 0 ? (deleteConfirm.permanent ? qsTr("%n message(s) will be destroyed. This cannot be undone.",
                                                                                                        "", deleteConfirm.uids.length) :
                                                                                                   qsTr("%n message(s) will be moved to Trash.",
                                                                                                        "", deleteConfirm.uids.length)) :
                                                                        (deleteConfirm.permanent ? qsTr(
                                                                                                       "“%1” will be destroyed. This cannot be undone.").arg(
                                                                                                       deleteConfirm.subject) :
                                                                                                   qsTr("“%1” will be moved to Trash.").arg(
                                                                                                       deleteConfirm.subject))
        }
    }

    Dialog {
        id: purgeConfirm
        title: qsTr("Delete permanently?")
        modal: true
        anchors.centerIn: parent
        // Clamped to the window, which may be narrower than the default.
        width: Math.min(420, root.width - 16)
        padding: Theme.lg

        property int uid: -1
        property var uids: []
        property string subject: ""

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        footer: RowLayout {
            spacing: Theme.sm
            Item {
                Layout.fillWidth: true
            }
            AppButton {
                text: qsTr("Cancel")
                onClicked: purgeConfirm.close()
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                Layout.topMargin: Theme.sm
                text: qsTr("Delete permanently")
                intent: "danger"
                onClicked: {
                    var targets = purgeConfirm.uids && purgeConfirm.uids.length > 0 ? purgeConfirm.uids.slice() :
                                                                                      [purgeConfirm.uid];
                    var bulk = purgeConfirm.uids && purgeConfirm.uids.length > 0;
                    purgeConfirm.close();
                    // Out of the click handler: purging rebuilds the feed.
                    Qt.callLater(function () {
                        if (bulk)
                            root.bulkPurge(targets);
                        else
                            root.purgeMessage(targets[0]);
                    });
                }
            }
        }

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            color: Theme.text
            font.pixelSize: Theme.fontBase
            text: purgeConfirm.uids && purgeConfirm.uids.length > 0 ? qsTr(
                                                                          "%n messages will be destroyed on the server. This cannot be undone.",
                                                                          "", purgeConfirm.uids.length) : qsTr(
                                                                          "“%1” will be destroyed on the server. This cannot be undone.").arg(
                                                                          purgeConfirm.subject)
        }
    }

    Settings {
        id: settingsDialog
        settingsBridge: appSettings
        backend: backend
        dbPath: backend.db_path
        onStatusMessage: text => {
            // The image setting changes what the feed sanitizes to, so the
            // open message must re-render from a fresh feed.
            reloadMessages();
            root.syncSettingsRevision++;
            root.statusText = text;
        }
    }

    Dialog {
        id: statusDetailsDialog
        title: qsTr("Status Details")
        modal: true
        anchors.centerIn: parent
        width: Math.min(640, root.width - 32)
        height: Math.min(380, root.height - 64)
        padding: Theme.lg

        background: Rectangle {
            color: Theme.bgRaised
            radius: Theme.radius
            border.color: Theme.border
            border.width: 1
        }

        footer: RowLayout {
            spacing: Theme.sm
            Item {
                Layout.fillWidth: true
            }
            AppButton {
                text: qsTr("Copy to Clipboard")
                intent: "primary"
                onClicked: {
                    root.copyToClipboard(root.statusText);
                    statusCopiedFeedback.start();
                }
            }
            AppButton {
                Layout.rightMargin: Theme.lg
                Layout.bottomMargin: Theme.md
                Layout.topMargin: Theme.sm
                text: qsTr("Close")
                onClicked: statusDetailsDialog.close()
            }
        }

        ColumnLayout {
            anchors.fill: parent
            spacing: Theme.sm

            Label {
                visible: statusCopiedFeedback.running
                text: qsTr("✓ Copied to clipboard")
                color: Theme.accent
                font.pixelSize: Theme.fontSmall
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                TextArea {
                    id: statusTextArea
                    text: root.statusText
                    color: Theme.text
                    font.pixelSize: Theme.fontBase
                    readOnly: true
                    selectByMouse: true
                    wrapMode: TextArea.WrapAnywhere
                    textFormat: TextArea.PlainText
                    background: Rectangle {
                        color: Theme.bgAlt
                        radius: Theme.radius
                        border.color: Theme.border
                        border.width: 1
                    }
                }
            }
        }
    }

    Timer {
        id: statusCopiedFeedback
        interval: 2000
        repeat: false
    }

    AppMenu {
        id: statusContextMenu
        AppMenuItem {
            label: qsTr("Copy to Clipboard")
            glyph: Icons.saveAlt
            onTriggered: root.copyToClipboard(root.statusText)
        }
        AppMenuItem {
            label: qsTr("View Details…")
            glyph: Icons.openInNew
            onTriggered: statusDetailsDialog.open()
        }
    }
}
