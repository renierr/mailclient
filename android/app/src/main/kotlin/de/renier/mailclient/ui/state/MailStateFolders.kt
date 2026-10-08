package de.renier.mailclient.ui.state

import de.renier.mailclient.MailNative
import de.renier.mailclient.MailShortcuts
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

// Account / folder / message-page operations for MailState: selection,
// opening, cache-first reads, sync queueing, folder management. Pure move
// from MailState.kt — call syntax (`state.selectAccount(…)`) is unchanged.

val MailState.activeAccount: Account? get() = accounts.firstOrNull { it.id == activeAccountId }
val MailState.openFolder: Folder? get() = folders.firstOrNull { it.id == folderId }

/** The sidebar and move picker: subscribed folders only. */
val MailState.visibleFolders: List<Folder> get() = folders.filter { it.subscribed }

fun MailState.refreshAll(syncAfter: Boolean = false, coldStart: Boolean = false) = io {
    MailNative.ensureInit(appContext)
    val parsed = JSONObject(MailNative.initialSelection())
    val accountId = parsed.optLong("account_id", -1)
    val landing = parsed.optLong("folder_id", -1)
    val loaded = parseAccounts(MailNative.accountsJson())
    withContext(Dispatchers.Main) {
        accounts = loaded
        activeAccountId = accountId
        // loadFolders() below clears it when the tree has no such folder.
        folderId = landing
    }
    MailShortcuts.update(appContext, loaded)
    loadFolders()
    if (coldStart) {
        val view = runCatching { JSONObject(MailNative.settingsJson()).optString("start_view") }.getOrNull()
        withContext(Dispatchers.Main) { startView = view ?: "folders" }
    }
    refreshOutbox()
    loadSort()
    if (folderId >= 0) reloadMessages()
    withContext(Dispatchers.Main) {
        // A running job's progress outranks the idle line.
        if (!busy) {
            status = if (accountId < 0) "No accounts yet" else "Ready"
            statusError = false
        }
    }
    // Accounts may have been added or removed: what runs in the
    // background follows.
    rescheduleBackground()
    if (accountId >= 0) {
        loadAutoSyncMinutes(accountId)
        withContext(Dispatchers.Main) { restartAutoSync() }
        if (syncAfter) syncAccount(accountId)
    }
}

fun MailState.selectAccount(id: Long) = io {
    withContext(Dispatchers.Main) { clearSearch() }
    MailNative.ensureInit(appContext)
    val parsed = JSONObject(MailNative.selectAccount(id))
    withContext(Dispatchers.Main) {
        activeAccountId = parsed.optLong("account_id", id)
        folderId = parsed.optLong("folder_id", -1)
        messages = emptyList()
    }
    loadFolders()
    refreshOutbox()
    if (folderId >= 0) reloadMessages()
    // A switch syncs the account it lands on, like Flutter; its own
    // interval replaces the previous account's timer.
    val landed = activeAccountId
    if (landed >= 0) {
        loadAutoSyncMinutes(landed)
        withContext(Dispatchers.Main) { restartAutoSync() }
        syncAccount(landed)
    }
}

internal fun MailState.loadFolders() = io {
    MailNative.ensureInit(appContext)
    val id = activeAccountId
    if (id < 0) {
        withContext(Dispatchers.Main) {
            folders = emptyList()
            refreshSidebarRows()
        }
        return@io
    }
    val tree = parseFolders(MailNative.foldersJson(id))
    withContext(Dispatchers.Main) {
        // Another account was selected meanwhile (a shortcut or notification
        // tap racing the cold-start load): its own load paints.
        if (activeAccountId != id) return@withContext
        folders = tree
        if (folderId >= 0 && tree.none { it.id == folderId }) {
            folderId = -1
            messages = emptyList()
        }
        refreshSidebarRows()
    }
}

fun MailState.refreshFolders(andMessages: Boolean = false) {
    loadFolders()
    if (andMessages) reloadMessages()
    refreshOutbox()
}

/** Flip a folder parent's collapse state in the sidebar tree. */
fun MailState.toggleFolderExpanded(id: Long) {
    if (!expandedFolders.remove(id)) expandedFolders.add(id)
    refreshSidebarRows()
}

/** Re-fold the sidebar for the current expanded set (main thread; local DB read). */
internal fun MailState.refreshSidebarRows() {
    val id = activeAccountId
    sidebarRows = if (id < 0) {
        emptyList()
    } else {
        val expanded = "[${expandedFolders.sorted().joinToString(",")}]"
        runCatching { parseSidebarRows(MailNative.sidebarRowsJson(id, expanded)) }
            .getOrDefault(emptyList())
    }
}

/** Cache-first open: paint cached rows at once, fill from the server after. */
fun MailState.openFolder(id: Long) {
    // Re-picking the shown folder just returns to its list, like Qt and
    // Flutter: no second fill of what is already showing.
    if (id == folderId && messages.isNotEmpty()) return
    exitSelectionMode()
    folderId = id
    messages = emptyList()
    shownMessages = emptyList()
    canLoadOlder = false
    olderState = ""
    olderLabel = ""
    reloadMessages()
    // A folder-scoped search follows the folder: fresh scope, fresh
    // server top-up (Qt `updateSearch` on a folder change).
    if (searchActive && searchFolderOnly && similarTarget == null) {
        serverSearchFired.set(false)
        serverSearchPending = false
        runSearch()
    }
    io {
        runCatching { MailNative.syncFolder(activeAccountId, id) }
            .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "sync failed") }
    }
}

internal fun MailState.reloadMessages() = io {
    MailNative.ensureInit(appContext)
    val id = folderId
    if (id < 0) return@io
    val counts = JSONObject(MailNative.folderCounts(id))
    // Every cached row, like Qt/Flutter: mail fetched with "Load older"
    // stays on the list. A fixed page would hide each older batch behind
    // the newest 200 forever.
    val cached = counts.optInt("cached", MailState.PAGE.toInt()).coerceAtLeast(1).toLong()
    val rows = parseMessages(MailNative.messagesJson(id, cached, 0))
    withContext(Dispatchers.Main) {
        if (folderId == id) {
            messages = rows
            val older = counts.optString("older")
            val cached = counts.optInt("cached", rows.size)
            val server = counts.optInt("server", -1)
            canLoadOlder = counts.optBoolean("can_load_older", false)
            olderState = older
            olderCounts = cached to server
            recomputeShown()
        }
    }
}

fun MailState.loadMore() = io {
    MailNative.ensureInit(appContext)
    runCatching { MailNative.loadOlderMessages(activeAccountId, folderId) }
        .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "load older failed") }
    // The finished event reloads the page; rows appear then.
}

fun MailState.syncNow() {
    val id = activeAccountId
    if (id < 0) {
        io { putStatus("No accounts yet", true) }
        return
    }
    syncAccount(id)
}

/**
 * Full sync of one account by id. Only queues: the busy line comes from
 * the core's in-flight table (see [busyKinds]). A second pull while the
 * same job runs is refused by the core's dedupe and ignored here — the
 * line already shows it.
 */
internal fun MailState.syncAccount(id: Long) = io {
    MailNative.ensureInit(appContext)
    runCatching { MailNative.syncAccount(id) }
        .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "sync failed") }
}

/**
 * Pull-to-refresh inside a folder syncs just that folder: new mail,
 * flags and pending moves for what is showing. The account-wide
 * [syncAccount] stays on the Sync entry and the folders pane.
 */
fun MailState.syncFolder(id: Long = folderId) = io {
    MailNative.ensureInit(appContext)
    val account = activeAccountId
    if (account < 0 || id < 0) return@io
    runCatching { MailNative.syncFolder(account, id) }
        .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "sync failed") }
}

/** Re-read the folder list from the server (LIST); the event reloads. */
fun MailState.refreshFolderList() {
    val id = activeAccountId
    if (id < 0) return
    io {
        runCatching { MailNative.refreshFolders(id) }
            .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "refresh failed") }
    }
}

/** Hide or show a folder in the sidebar. Display-only. */
fun MailState.setFolderSubscribed(id: Long, subscribed: Boolean) = io {
    MailNative.ensureInit(appContext)
    MailNative.setFolderSubscribed(id, subscribed)
    loadFolders()
}

/**
 * Create [path] on the server (`/` nests; the core maps it onto the
 * account delimiter). [onDone] gets (ok, status) when the job finishes,
 * or straight away when it could not be queued.
 */
fun MailState.createFolder(path: String, onDone: (Boolean, String) -> Unit) {
    val id = activeAccountId
    if (id < 0) {
        onDone(false, "No account")
        return
    }
    finishWaiters.getOrPut("Folders") { mutableListOf() }.add(onDone)
    io {
        runCatching { MailNative.createFolder(id, path) }
            .onFailure { e ->
                withContext(Dispatchers.Main) {
                    finishWaiters["Folders"]?.remove(onDone)
                    onDone(false, e.message ?: "Could not create the folder")
                }
            }
    }
}

/** Move one message of [fromFolder] to [destPath]; offers undo. */
fun MailState.moveMessage(fromFolder: Long, uid: Int, destPath: String) = io {
    MailNative.ensureInit(appContext)
    val result = MailNative.moveMessages(activeAccountId, fromFolder, "[$uid]", destPath)
    withContext(Dispatchers.Main) {
        offerUndo(result)
        refreshSearch()
    }
    reloadMessages()
    loadFolders()
}

/** After the reader changed a message: re-read list, tree and search. */
fun MailState.afterReaderChange() {
    reloadMessages()
    loadFolders()
    refreshSearch()
}
