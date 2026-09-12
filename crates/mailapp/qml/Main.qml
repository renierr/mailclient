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
    minimumWidth: 720
    minimumHeight: 460
    title: qsTr("Mailclient")
    color: Theme.bg

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
    property bool busy: false

    Bridge {
        id: backend
    }

    SettingsBridge {
        id: appSettings
    }

    ListModel { id: folderModel }
    ListModel { id: accountModel }

    // The message feed is kept as plain JavaScript objects, not a ListModel.
    // `ListModel.get()` hands out QObjects the model owns, so the reader pane
    // holding the selected message was dereferencing freed memory the moment a
    // reload cleared the model -- the segfault on repeated clicks. Plain
    // objects are snapshots and stay valid.
    property var messageRows: []

    // --- feed plumbing ----------------------------------------------------

    function reloadFolders() {
        // Synced in place, never cleared: clearing destroys every sidebar
        // delegate on every click (see qml/ModelSync.qml).
        ModelSync.sync(folderModel, JSON.parse(backend.folders_json), "name")
        // The sidebar shows a subscribed-only subset, and in-place row edits
        // don't re-fire `onFoldersChanged` — refresh the subset explicitly.
        sidebar.refreshShown()
        // Keep selection if still present, else inbox, else first.
        var found = false
        for (var j = 0; j < folderModel.count; j++) {
            if (folderModel.get(j).name === root.currentFolder) {
                found = true
                break
            }
        }
        if (!found) {
            var inbox = ""
            for (var k = 0; k < folderModel.count; k++) {
                if (folderModel.get(k).role === "inbox")
                    inbox = folderModel.get(k).name
            }
            root.currentFolder = inbox !== "" ? inbox : (folderModel.count > 0 ? folderModel.get(0).name : "")
        }
    }

    function reloadMessages() {
        root.messageRows = JSON.parse(backend.messages_json)
        // Drop the selection only if that message really is gone.
        if (root.messageByUid(root.currentUid) === undefined)
            root.currentUid = -1
    }

    function reloadAccounts() {
        ModelSync.sync(accountModel, JSON.parse(backend.accounts_json), "id")
    }

    function reloadAll() {
        var r = backend.refresh_accounts()
        reloadAccounts()
        reloadFolders()
        reloadMessages()
        return r
    }

    function messageByUid(uid) {
        if (uid < 0)
            return undefined
        for (var i = 0; i < root.messageRows.length; i++) {
            if (root.messageRows[i].uid === uid)
                return root.messageRows[i]
        }
        return undefined
    }

    function showResult(okMessage, result) {
        root.statusText = result === "" ? okMessage : result
    }

    // --- actions ----------------------------------------------------------

    function openMessage(uid) {
        if (uid < 0 || uid === root.currentUid)
            return  // already open: re-clicking a row must not reload anything
        for (var i = 0; i < folderModel.count; i++) {
            if (folderModel.get(i).name === root.currentFolder
                    && folderModel.get(i).role === "drafts") {
                var draft = JSON.parse(backend.draft_form(uid))
                if (draft.draft_uid === undefined) {
                    root.statusText = qsTr("Draft is no longer available")
                } else {
                    composer.openForDraft(draft)
                }
                return
            }
        }
        root.currentUid = uid
        markReadTimer.stop()
        if (!appSettings.auto_mark_read) {
            return  // stay unread until the user says otherwise
        }
        if (appSettings.mark_read_delay_secs <= 0) {
            markAsRead(uid)
        } else {
            // Thunderbird-style: counts as read only if still viewing it
            // when the delay elapses; moving on keeps it unread.
            markReadTimer.uid = uid
            markReadTimer.interval = appSettings.mark_read_delay_secs * 1000
            markReadTimer.start()
        }
    }

    function markAsRead(uid) {
        if (uid < 0)
            return
        // Local-only mark-as-read: fast, no network on the click path.
        var r = backend.open_message(uid)
        reloadFolders()
        reloadMessages()
        if (r !== "")
            root.statusText = r
    }

    function syncNow() {
        if (backend.account_count === 0) {
            root.statusText = qsTr("Add an account first")
            return
        }
        root.busy = true
        root.statusText = qsTr("Syncing…")
        // NOTE: blocking network call; async worker is a follow-up.
        var r = backend.sync_now()
        reloadFolders()
        reloadMessages()
        root.busy = false
        root.statusText = r
    }

    // One older batch (200) below the oldest cached UID, then the page grows
    // so the list extends backwards without losing scroll position (the
    // models update in place — see ModelSync).
    function loadOlder() {
        if (root.busy)
            return
        root.busy = true
        root.statusText = qsTr("Loading older messages…")
        var r = backend.load_older_messages()
        reloadFolders()
        reloadMessages()
        root.busy = false
        root.statusText = r
    }

    function toggleStar(uid) {
        if (uid < 0)
            return
        showResult("", backend.toggle_star(uid))
        reloadMessages()
    }

    // Moves to Trash — except spam (destroyed outright, junk never touches
    // Trash) and Trash itself (deleting there is permanent). The bridge
    // reports which it did because "moved" and "destroyed" differ.
    function deleteMessage(uid) {
        if (uid < 0)
            return
        var r = backend.delete_message(uid)
        if (root.currentUid === uid)
            root.currentUid = -1
        reloadFolders()
        reloadMessages()
        root.statusText = r === "" ? qsTr("Deleted") : r
    }

    // One-click archive: moves to the Archive folder (created on demand).
    function archiveMessage(uid) {
        if (uid < 0)
            return
        var r = backend.archive_message(uid)
        if (root.currentUid === uid)
            root.currentUid = -1
        reloadFolders()
        reloadMessages()
        root.statusText = r === "" ? qsTr("Archived") : r
    }

    // Move picker: remembers which message, the dialog reports the target.
    function openMove(uid) {
        if (uid < 0)
            return
        var m = root.messageByUid(uid)
        moveDialog.uid = uid
        moveDialog.subject = m !== undefined ? m.subject : ""
        moveDialog.open()
    }

    function purgeMessage(uid) {
        if (uid < 0)
            return
        var r = backend.purge_message(uid)
        if (root.currentUid === uid)
            root.currentUid = -1
        reloadFolders()
        reloadMessages()
        root.statusText = r === "" ? qsTr("Deleted permanently") : r
    }

    function confirmPurge(uid) {
        if (uid < 0)
            return
        var m = root.messageByUid(uid)
        purgeConfirm.uid = uid
        purgeConfirm.subject = m !== undefined ? m.subject : ""
        purgeConfirm.open()
    }

    function selectFolder(path) {
        var r = backend.select_folder(path)
        if (r === "") {
            markReadTimer.stop()
            root.currentFolder = path
            root.currentUid = -1
            reloadMessages()
            // On-demand fill: auto-sync only covered this folder quickly
            // (newest 50), so fetch its newest 200 now that it is open.
            // Single-folder + windowed, still cheap; failure keeps the cache.
            var s = backend.sync_folder_now(path)
            reloadFolders()
            reloadMessages()
            root.statusText = s === "" || s.indexOf("Synced") === 0
                ? qsTr("Folder: %1").arg(path)
                : s
        } else {
            root.statusText = r
        }
    }

    function selectAccount(id) {
        var r = backend.select_account(id)
        if (r === "") {
            markReadTimer.stop()
            root.currentUid = -1
            root.currentFolder = ""
            reloadAccounts()
            reloadFolders()
            reloadMessages()
            root.statusText = qsTr("Account: %1").arg(backend.current_account_email)
        } else {
            root.statusText = r
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
        function onColorSchemeChanged() { backend.apply_native_theme(Theme.dark) }
    }

    // Delayed mark-as-read: fires only while the same message is still open.
    Timer {
        id: markReadTimer
        property int uid: -1
        repeat: false
        onTriggered: {
            if (markReadTimer.uid >= 0 && markReadTimer.uid === root.currentUid)
                root.markAsRead(markReadTimer.uid)
        }
    }

    Component.onCompleted: {
        appSettings.load()
        var r = reloadAll()
        if (backend.account_count === 0) {
            root.statusText = qsTr("Add an account to start")
            accountSetup.openNew()
        } else if (r !== "") {
            root.statusText = r
        } else {
            root.statusText = qsTr("Ready")
            // Refresh on startup: show the cache immediately, then sync.
            // Deferred so first paint happens first (sync blocks on network).
            Qt.callLater(function () {
                if (backend.account_count > 0)
                    root.syncNow()
            })
        }
    }

    // --- keyboard ---------------------------------------------------------

    Shortcut { sequences: ["Ctrl+N"]; onActivated: composer.openBlank() }
    Shortcut { sequences: ["Ctrl+R", "F5"]; onActivated: root.syncNow() }
    Shortcut { sequences: ["Ctrl+F"]; onActivated: searchField.forceActiveFocus() }
    Shortcut { sequences: ["Down"]; onActivated: messageList.step(1) }
    Shortcut { sequences: ["Up"]; onActivated: messageList.step(-1) }
    Shortcut { sequences: ["Delete"]; onActivated: root.deleteMessage(root.currentUid) }
    Shortcut { sequences: ["Shift+Delete"]; onActivated: root.confirmPurge(root.currentUid) }
    Shortcut { sequences: ["S"]; onActivated: root.toggleStar(root.currentUid) }
    Shortcut { sequences: ["A"]; onActivated: root.archiveMessage(root.currentUid) }
    Shortcut { sequences: ["M"]; onActivated: root.openMove(root.currentUid) }
    Shortcut {
        sequences: ["R"]
        onActivated: if (root.currentUid >= 0) composer.openForReply(root.messageByUid(root.currentUid))
    }
    Shortcut {
        sequences: ["F"]
        onActivated: if (root.currentUid >= 0) composer.openForForward(root.messageByUid(root.currentUid))
    }

    // --- chrome -----------------------------------------------------------

    header: Rectangle {
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
                text: "☰"
                tooltip: qsTr("Toggle sidebar")
                onClicked: sidebar.visible = !sidebar.visible
            }

            AppButton {
                text: qsTr("✎  Compose")
                intent: "primary"
                enabled: backend.account_count > 0
                onClicked: composer.openBlank()
            }

            // Live filter over the loaded feed (server-side FTS is M3).
            TextField {
                id: searchField
                Layout.fillWidth: true
                Layout.maximumWidth: 460
                implicitHeight: 32
                placeholderText: qsTr("Search sender, subject or snippet…")
                color: Theme.text
                placeholderTextColor: Theme.textMuted
                font.pixelSize: Theme.fontBase
                leftPadding: Theme.sm
                rightPadding: clearSearch.visible ? 28 : Theme.sm
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
                    width: 24
                    height: 24
                    visible: searchField.text !== ""
                    text: "✕"
                    fontSize: Theme.fontSmall
                    tooltip: qsTr("Clear search")
                    onClicked: searchField.text = ""
                }
                Keys.onEscapePressed: searchField.text = ""
            }

            Item { Layout.fillWidth: true }

            IconButton {
                text: "⟳"
                tooltip: qsTr("Sync now (Ctrl+R)")
                enabled: backend.account_count > 0 && !root.busy
                onClicked: root.syncNow()
            }
            IconButton {
                text: "🗂"
                tooltip: qsTr("Manage IMAP folders")
                enabled: backend.account_count > 0
                onClicked: foldersDialog.open()
            }
            IconButton {
                text: "✉"
                tooltip: qsTr("Accounts")
                onClicked: accountsDialog.open()
            }
            IconButton {
                text: "@"
                tooltip: qsTr("Contacts")
                onClicked: contactsDialog.open()
            }
            IconButton {
                text: "⚙"
                tooltip: qsTr("Settings")
                onClicked: settingsDialog.open()
            }
        }
    }

    SplitView {
        anchors.fill: parent

        handle: Rectangle {
            implicitWidth: 1
            color: SplitHandle.pressed || SplitHandle.hovered ? Theme.accent : Theme.border
        }

        Sidebar {
            id: sidebar
            SplitView.preferredWidth: 250
            SplitView.minimumWidth: 160
            folders: folderModel
            accounts: accountModel
            currentFolder: root.currentFolder
            currentEmail: backend.current_account_email
            currentAccountId: backend.current_account_id
            onFolderSelected: path => root.selectFolder(path)
            onAccountSelected: id => root.selectAccount(id)
            onAddAccountRequested: accountSetup.openNew()
            onManageAccountsRequested: accountsDialog.open()
            onManageFoldersRequested: foldersDialog.open()
        }

        MessageList {
            id: messageList
            SplitView.preferredWidth: 360
            SplitView.minimumWidth: 240
            messages: root.messageRows
            currentUid: root.currentUid
            folderName: root.currentFolder
            filterText: searchField.text
            totalCount: backend.messages_total
            limit: backend.message_limit
            busy: root.busy
            onMessageSelected: uid => root.openMessage(uid)
            onStarToggled: uid => root.toggleStar(uid)
            onArchiveRequested: uid => root.archiveMessage(uid)
            onMoveRequested: uid => root.openMove(uid)
            onMarkReadRequested: (uid, read) => {
                var r = backend.mark_read(uid, read)
                reloadFolders()
                reloadMessages()
                root.statusText = r !== "" ? r : (read ? qsTr("Marked as read") : qsTr("Marked as unread"))
            }
            onDeleteRequested: uid => root.deleteMessage(uid)
            onPurgeRequested: uid => root.confirmPurge(uid)
        }

        MessageView {
            id: messageView
            SplitView.fillWidth: true
            SplitView.minimumWidth: 260
            loadRemoteImages: appSettings.load_remote_images
            backend: backend
            message: root.messageByUid(root.currentUid)
            onReplyRequested: composer.openForReply(root.messageByUid(root.currentUid))
            onReplyAllRequested: composer.openForReply(root.messageByUid(root.currentUid))
            onForwardRequested: composer.openForForward(root.messageByUid(root.currentUid))
            onStarRequested: root.toggleStar(root.currentUid)
            onArchiveRequested: root.archiveMessage(root.currentUid)
            onMoveRequested: root.openMove(root.currentUid)
            onDeleteRequested: root.deleteMessage(root.currentUid)
            onStatusMessage: text => root.statusText = text
        }
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
                text: root.busy ? "⟳" : ""
                color: Theme.accent
                font.pixelSize: Theme.fontSmall
            }
            Label {
                Layout.fillWidth: true
                text: root.statusText
                color: Theme.textMuted
                font.pixelSize: Theme.fontSmall
                elide: Text.ElideRight
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
            var r = backend.send_mail(payload)
            if (r === "") {
                composer.markClean()
                composer.close()
                reloadFolders()
                reloadMessages()
                root.statusText = qsTr("Sent")
            } else if (r.indexOf("sent, but") === 0) {
                // SMTP already accepted the message. Closing prevents a retry
                // from sending a duplicate while keeping the source draft for recovery.
                composer.markClean()
                composer.close()
                reloadFolders()
                reloadMessages()
                root.statusText = r
            } else {
                root.statusText = r
            }
        }
        onSaveDraftRequested: payload => {
            var r = backend.save_draft(payload)
            if (r === "") {
                composer.markClean()
                composer.close()
                reloadFolders()
                reloadMessages()
                root.statusText = qsTr("Draft saved")
            } else if (r.indexOf("draft saved, but") === 0) {
                // A replacement was appended but its old source survived.
                // Close so retrying cannot append another duplicate.
                composer.markClean()
                composer.close()
                reloadFolders()
                reloadMessages()
                root.statusText = r
            } else {
                root.statusText = r
            }
        }
    }

    AccountSetup {
        id: accountSetup
        onStatusMessage: text => root.statusText = text
        onAccountSubmit: payload => {
            var r = backend.add_account(payload)
            if (r === "") {
                var wasEditing = accountSetup.editing
                accountSetup.close()
                reloadAll()
                root.statusText = wasEditing ? qsTr("Account updated")
                                             : qsTr("Account added — press ⟳ to sync")
            } else {
                root.statusText = r
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
            var r = backend.delete_account(id)
            reloadAll()
            showResult(qsTr("Account removed"), r)
        }
    }

    Contacts {
        id: contactsDialog
        backend: backend
        onStatusMessage: text => root.statusText = text
    }

    Folders {
        id: foldersDialog
        folders: folderModel
        currentFolder: root.currentFolder
        busy: root.busy
        onStatusMessage: text => root.statusText = text
        onRefreshRequested: {
            if (root.busy)
                return
            root.busy = true
            root.statusText = qsTr("Refreshing folders…")
            var r = backend.refresh_folders()
            reloadFolders()
            reloadMessages()
            root.busy = false
            showResult(qsTr("Folders refreshed"), r)
        }
        onVisibilityToggled: (path, subscribed) => {
            showResult("", backend.set_folder_subscribed(path, subscribed))
            reloadFolders()
        }
        onCreateRequested: path => {
            var r = backend.create_folder(path)
            reloadFolders()
            reloadMessages()
            foldersDialog.clearNewFolder()
            showResult(qsTr("Folder created"), r)
        }
        onFolderSelected: path => {
            foldersDialog.close()
            // Out of the click handler: selecting rebuilds the feed.
            Qt.callLater(root.selectFolder, path)
        }
    }

    MoveTo {
        id: moveDialog
        folders: folderModel
        currentFolder: root.currentFolder
        onFolderChosen: path => {
            var target = moveDialog.uid
            moveDialog.close()
            // Out of the click handler: moving rebuilds the feed.
            Qt.callLater(function () {
                var r = backend.move_message(target, path)
                if (root.currentUid === target)
                    root.currentUid = -1
                reloadFolders()
                reloadMessages()
                root.statusText = r === "" ? qsTr("Moved") : r
            })
        }
    }

    Dialog {
        id: purgeConfirm
        title: qsTr("Delete permanently?")
        modal: true
        anchors.centerIn: parent
        width: 420
        padding: Theme.lg

        property int uid: -1
        property string subject: ""

        background: Rectangle {
            color: Theme.bg
            radius: Theme.radiusLg
            border.width: 1
            border.color: Theme.border
        }

        footer: RowLayout {
            spacing: Theme.sm
            Item { Layout.fillWidth: true }
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
                    var target = purgeConfirm.uid
                    purgeConfirm.close()
                    // Out of the click handler: purging rebuilds the feed.
                    Qt.callLater(root.purgeMessage, target)
                }
            }
        }

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            color: Theme.text
            font.pixelSize: Theme.fontBase
            text: qsTr("“%1” will be destroyed on the server. This cannot be undone.")
                  .arg(purgeConfirm.subject)
        }
    }

    Settings {
        id: settingsDialog
        settingsBridge: appSettings
        dbPath: backend.db_path
        onStatusMessage: text => {
            // The image setting changes what the feed sanitizes to, so the
            // open message must re-render from a fresh feed.
            reloadMessages()
            root.statusText = text
        }
    }
}
