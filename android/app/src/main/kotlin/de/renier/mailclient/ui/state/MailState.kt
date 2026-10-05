package de.renier.mailclient.ui.state

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import de.renier.mailclient.JobEvents
import de.renier.mailclient.MailNative
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

// Step 1 shell state: what the app shows and what it does, over the 0a–0e
// JNI surface. Plain holder (no new dependencies — no ViewModel, no
// navigation-compose), owned by MailShell's composition. Reads take explicit
// ids and jobs only say *that* something changed, so this re-reads whatever
// is showing — the same contract the Dart MailState keeps.
data class Account(
    val id: Long,
    val email: String,
    val name: String,
    val initials: String = "?",
    val avatarLight: String = "",
    val avatarDark: String = "",
)

data class Folder(
    val id: Long,
    val path: String,
    val leaf: String,
    val depth: Int,
    val role: String,
    val unread: Int,
    val count: Int,
    // Sidebar visibility only: hidden folders keep their cache and syncing.
    val subscribed: Boolean = true,
    // Delete here destroys instead of moving to Trash (core decides).
    val deleteIsPermanent: Boolean = false,
)

// The settings the reader acts on, from the core's settingsJson.
data class ReaderPrefs(
    val autoMarkRead: Boolean = true,
    val markReadDelaySecs: Long = 0,
    val loadRemoteImages: Boolean = false,
    val confirmDelete: Boolean = true,
    val linkClickAction: String = "examine",
    // Text size multiplier: the Qt reader's 12 / 14 / 18 px steps.
    val scale: Float = 1f,
)

data class MessageRow(
    val uid: Int,
    val subject: String,
    val from: String,
    val fromName: String,
    val date: String,
    val snippet: String,
    val unread: Boolean,
    val starred: Boolean,
    val hasAttachments: Boolean,
    // Raw UTC timestamp for the date quick-filter (`dateFilterMatches`);
    // `date` above is display text.
    val dateRaw: String = "",
    // Core-decided avatar (mailcore::badge): initials + per-theme hex.
    val initials: String = "?",
    val avatarLight: String = "",
    val avatarDark: String = "",
    // Search hits span folders and carry their own; list rows leave -1.
    val folderId: Long = -1,
)

data class UndoOffer(val batch: String, val label: String)

class MailState(private val appContext: Context, private val scope: CoroutineScope) {
    // Every UI-read field is snapshot state: plain vars never recompose.
    // All writes already hop to Dispatchers.Main (see io/putStatus).
    var initialized by mutableStateOf(false)
        private set
    private var jobEvents: AutoCloseable? = null
    var accounts: List<Account> by mutableStateOf(emptyList())
        private set
    var activeAccountId by mutableStateOf(-1L)
        private set
    var folders: List<Folder> by mutableStateOf(emptyList())
        private set
    var folderId by mutableStateOf(-1L)
        private set
    var messages: List<MessageRow> by mutableStateOf(emptyList())
        private set
    var canLoadOlder by mutableStateOf(false)
        private set
    // The load-older footer's words ("Cached 200 (server not checked)",
    // "Cached 200 of 350", "All 350 loaded") plus its raw state; hidden
    // when the server holds nothing ("empty"), like the desktop footer.
    var olderState by mutableStateOf("")
        private set
    var olderLabel by mutableStateOf("")
        private set
    var status by mutableStateOf("Starting…")
        private set
    var statusError by mutableStateOf(false)
        private set
    var syncing by mutableStateOf(false)
        private set
    var outboxPending by mutableStateOf(0)
        private set
    var outboxFailed by mutableStateOf(false)
        private set
    var undoOffer: UndoOffer? by mutableStateOf(null)
        private set
    var notice: String? by mutableStateOf(null)
        private set
    // A "Folders" job (LIST refresh, create) is queued or running.
    var foldersBusy by mutableStateOf(false)
        private set

    // Search: the typed text, whether it is scoped to the open folder, and
    // the rows it found. How a query runs (off / row filter / FTS index,
    // debounce) is the core's call via searchPlan.
    var searchQuery by mutableStateOf("")
        private set
    var searchFolderOnly by mutableStateOf(false)
        private set
    var searchHits: List<MessageRow> by mutableStateOf(emptyList())
        private set
    var searchActive by mutableStateOf(false)
        private set
    // "Similar to: …" when the hits are a find-similar result, not a query.
    var similarLabel: String? by mutableStateOf(null)
        private set
    // A server backfill is in flight for the current query (thin index
    // results, like Qt/Flutter); the Search job's finish re-runs the query.
    var serverSearchPending by mutableStateOf(false)
        private set
    private var serverSearchFired = false
    // Step 4 list state: sort (persisted in core settings), AND-combined
    // quick filters, and multi-select. The core sorts the page itself; the
    // filters apply client-side to loaded rows and search hits alike, like
    // the Flutter list — they never fetch.
    var sortField by mutableStateOf("date")
        private set
    var sortDesc by mutableStateOf(true)
        private set
    var filterUnread by mutableStateOf(false)
        private set
    var filterStarred by mutableStateOf(false)
        private set
    var filterAttachments by mutableStateOf(false)
        private set
    // YYYY-MM-DD day bounds, After inclusive / Before exclusive, "" = unset.
    var filterAfter by mutableStateOf("")
        private set
    var filterBefore by mutableStateOf("")
        private set
    // Words for the active date filter ("Today", "Mar 3 – Mar 9", …).
    var dateFilterLabel by mutableStateOf("")
        private set
    var selectionMode by mutableStateOf(false)
        private set
    // Folder rows key by uid; search hits span folders, keyed folderId:uid.
    var selectedKeys: Set<String> by mutableStateOf(emptySet())
        private set
    // Loaded rows minus the quick filters; what the list actually paints.
    var shownMessages: List<MessageRow> by mutableStateOf(emptyList())
        private set
    var shownHits: List<MessageRow> by mutableStateOf(emptyList())
        private set
    // List scroll memory (Qt keeps per-folder scroll by UID, Flutter by
    // PageStorageKey): first-visible index + offset per folder, so leaving
    // for the reader and coming back lands where you left. Search scroll
    // is keyed separately and likewise restored.
    private val listScroll = mutableMapOf<String, Pair<Int, Int>>()

    fun saveListScroll(key: String, index: Int, offset: Int) {
        listScroll[key] = index to offset
    }

    fun listScrollFor(key: String): Pair<Int, Int>? = listScroll[key]
    var readerPrefs by mutableStateOf(ReaderPrefs())
        private set
    private var searchJob: Job? = null

    // One-shot callbacks for the next finished event of a job kind, keyed by
    // kind. Main thread only (registered and drained there).
    private val finishWaiters = mutableMapOf<String, MutableList<(Boolean, String) -> Unit>>()

    val activeAccount: Account? get() = accounts.firstOrNull { it.id == activeAccountId }
    val openFolder: Folder? get() = folders.firstOrNull { it.id == folderId }

    /** The sidebar and move picker: subscribed folders only. */
    val visibleFolders: List<Folder> get() = folders.filter { it.subscribed }

    fun info(msg: String) {
        notice = msg
    }

    fun consumeNotice() {
        notice = null
    }

    fun dismissUndo() {
        undoOffer = null
    }

    private fun io(work: suspend () -> Unit) {
        scope.launch(Dispatchers.IO) {
            try {
                work()
            } catch (e: Exception) {
                fail(e.message ?: "failed")
            }
        }
    }

    private suspend fun fail(msg: String) {
        withContext(Dispatchers.Main) {
            status = msg
            statusError = true
            syncing = false
        }
    }

    private suspend fun putStatus(text: String, error: Boolean) {
        withContext(Dispatchers.Main) {
            status = text
            statusError = error
        }
    }

    /** First load + job-event listener. Idempotent for the composition. */
    fun ensureInit() {
        if (jobEvents == null) {
            jobEvents =
                JobEvents.subscribe(appContext) { json ->
                    scope.launch(Dispatchers.Main) { onJobEvent(json) }
                }
        }
        if (initialized) {
            refreshAll()
            return
        }
        initialized = true
        MailNative.ensureInit(appContext)
        refreshAll()
        loadReaderPrefs()
    }

    fun release() {
        jobEvents?.close()
        jobEvents = null
    }


    private fun onJobEvent(json: String) {
        val e = runCatching { JSONObject(json) }.getOrNull() ?: return
        val kind = e.optString("kind")
        val ok = e.optBoolean("ok", true)
        status = e.optString("status", kind)
        statusError = !ok
        if (e.optString("phase") != "finished") return
        if (kind == "Sync") syncing = false
        if (kind == "Folders") foldersBusy = false
        // A finished server backfill lands via the re-run below.
        if (kind == "Search") serverSearchPending = false
        finishWaiters.remove(kind)?.forEach { it(ok, e.optString("status")) }
        val accountId = e.optLong("account_id", -1)
        val eventFolder = e.optLong("folder_id", -1)
        // Re-read whatever is showing, like the Dart side does.
        if (kind == "Folders" || (accountId == activeAccountId && eventFolder == -1L)) {
            refreshFolders(andMessages = true)
        } else if (accountId == activeAccountId && (eventFolder == folderId || eventFolder == -1L)) {
            reloadMessages()
        }
        refreshOutbox()
        if (searchActive && similarLabel == null) runSearch()
    }

    fun refreshAll() = io {
        MailNative.ensureInit(appContext)
        val parsed = JSONObject(MailNative.initialSelection())
        val accountId = parsed.optLong("account_id", -1)
        val landing = parsed.optLong("folder_id", -1)
        withContext(Dispatchers.Main) {
            accounts = parseAccounts(MailNative.accountsJson())
            activeAccountId = accountId
            // loadFolders() below clears it when the tree has no such folder.
            folderId = landing
        }
        loadFolders()
        refreshOutbox()
        loadSort()
        if (folderId >= 0) reloadMessages()
        putStatus(if (accountId < 0) "No accounts yet" else "Ready", false)
    }

    // ---- Step 4: sort, filters, selection, bulk actions ----

    /** Sort the list; hidden while searching (hits stay newest-first). */
    fun setSort(field: String, descending: Boolean) = io {
        MailNative.ensureInit(appContext)
        MailNative.setSort(field, descending)
        loadSort()
        reloadMessages()
    }

    private fun loadSort() = io {
        MailNative.ensureInit(appContext)
        val o = runCatching { JSONObject(MailNative.settingsJson()) }.getOrDefault(JSONObject())
        withContext(Dispatchers.Main) {
            sortField = o.optString("message_sort_field", "date").ifEmpty { "date" }
            sortDesc = o.optBoolean("message_sort_desc", true)
        }
    }

    val hasDateFilter: Boolean get() = filterAfter.isNotEmpty() || filterBefore.isNotEmpty()
    val hasListFilter: Boolean
        get() = filterUnread || filterStarred || filterAttachments || hasDateFilter

    fun setUnreadOnly(only: Boolean) {
        filterUnread = only
        recomputeShown()
    }

    fun setStarredOnly(only: Boolean) {
        filterStarred = only
        recomputeShown()
    }

    fun setAttachmentsOnly(only: Boolean) {
        filterAttachments = only
        recomputeShown()
    }

    fun setAfterDay(day: String) {
        filterAfter = day
        refreshDateLabel()
        recomputeShown()
    }

    fun setBeforeDay(day: String) {
        filterBefore = day
        refreshDateLabel()
        recomputeShown()
    }

    /** A `today` / `week` / `month` / `older_month` preset from the core. */
    fun applyDatePreset(preset: String) {
        val o = runCatching { JSONObject(MailNative.datePresetRange(preset)) }.getOrDefault(JSONObject())
        filterAfter = o.optString("after")
        filterBefore = o.optString("before")
        refreshDateLabel()
        recomputeShown()
    }

    fun clearDateFilter() {
        filterAfter = ""
        filterBefore = ""
        dateFilterLabel = ""
        recomputeShown()
    }

    fun clearListFilters() {
        filterUnread = false
        filterStarred = false
        filterAttachments = false
        clearDateFilter()
    }

    private fun refreshDateLabel() {
        dateFilterLabel =
            if (hasDateFilter) MailNative.dateFilterLabel(filterAfter, filterBefore) else ""
    }

    /** AND-combined quick filters over one row; never fetches. */
    private fun rowShown(m: MessageRow): Boolean {
        if (filterUnread && !m.unread) return false
        if (filterStarred && !m.starred) return false
        if (filterAttachments && !m.hasAttachments) return false
        if (hasDateFilter &&
            MailNative.dateFilterMatches(m.dateRaw, filterAfter, filterBefore) != "true"
        ) {
            return false
        }
        return true
    }

    /** What the list paints; the backing rows stay untouched. */
    private fun recomputeShown() {
        shownMessages = messages.filter { rowShown(it) }
        shownHits = searchHits.filter { rowShown(it) }
        pruneSelection()
    }

    /** Rows of the active pane (folder or search), after filters. */
    fun visibleRows(): List<MessageRow> = if (searchActive) shownHits else shownMessages

    fun selectionKey(m: MessageRow): String =
        if (searchActive) "${if (m.folderId >= 0) m.folderId else folderId}:${m.uid}"
        else m.uid.toString()

    fun enterSelectionMode(withKey: String? = null) {
        selectionMode = true
        selectedKeys = if (withKey != null) setOf(withKey) else emptySet()
    }

    fun exitSelectionMode() {
        selectionMode = false
        selectedKeys = emptySet()
    }

    fun toggleSelected(key: String) {
        selectedKeys = if (key in selectedKeys) selectedKeys - key else selectedKeys + key
    }

    fun selectAllVisible() {
        selectedKeys = visibleRows().map { selectionKey(it) }.toSet()
    }

    fun selectUnreadVisible() {
        selectedKeys = visibleRows().filter { it.unread }.map { selectionKey(it) }.toSet()
    }

    fun selectStarredVisible() {
        selectedKeys = visibleRows().filter { it.starred }.map { selectionKey(it) }.toSet()
    }

    fun invertSelection() {
        val all = visibleRows().map { selectionKey(it) }.toSet()
        selectedKeys = all - selectedKeys
    }

    /** Drop keys that are no longer on screen (folder change, sync, filter). */
    private fun pruneSelection() {
        if (selectedKeys.isEmpty()) return
        val live = visibleRows().map { selectionKey(it) }.toSet()
        selectedKeys = selectedKeys.intersect(live)
        if (selectedKeys.isEmpty()) selectionMode = false
    }

    val selectionCount: Int get() = selectedKeys.size

    /** The selected rows, resolved against the active pane. */
    private fun selectedRows(): List<MessageRow> {
        val keys = selectedKeys
        if (keys.isEmpty()) return emptyList()
        return visibleRows().filter { selectionKey(it) in keys }
    }

    val selectionAllStarred: Boolean get() = selectedRows().all { it.starred }

    /** Delete here destroys instead of moving to Trash (core decides). */
    val selectionDeleteIsPermanent: Boolean
        get() {
            val rows = selectedRows()
            if (rows.isEmpty()) return false
            return rows.all { rowFolder(it)?.deleteIsPermanent == true }
        }

    /** The folder a row lives in: hits carry their own, rows use the open one. */
    private fun rowFolder(m: MessageRow): Folder? {
        val id = if (searchActive) {
            if (m.folderId >= 0) m.folderId else folderId
        } else {
            folderId
        }
        return folders.firstOrNull { it.id == id }
    }

    private fun selectionUidsJson(): String =
        "[${selectedRows().joinToString(",") { it.uid.toString() }}]"

    /** `[{"folder": path, "uid": n}]` for the `*Hits` bulk calls. */
    private fun selectionHitsJson(): String {
        val arr = org.json.JSONArray()
        for (m in selectedRows()) {
            val path = rowFolder(m)?.path ?: continue
            arr.put(JSONObject().put("folder", path).put("uid", m.uid))
        }
        return arr.toString()
    }

    fun bulkMarkRead(read: Boolean) = io {
        MailNative.ensureInit(appContext)
        if (searchActive) {
            MailNative.markReadHits(activeAccountId, selectionHitsJson(), read)
        } else {
            MailNative.markReadMany(activeAccountId, folderId, selectionUidsJson(), read)
        }
        afterBulk()
    }

    fun bulkStar(starred: Boolean) = io {
        MailNative.ensureInit(appContext)
        if (searchActive) {
            MailNative.setStarHits(activeAccountId, selectionHitsJson(), starred)
        } else {
            MailNative.setStarMany(activeAccountId, folderId, selectionUidsJson(), starred)
        }
        afterBulk()
    }

    fun bulkArchive() = io {
        MailNative.ensureInit(appContext)
        val result = if (searchActive) {
            MailNative.archiveHits(activeAccountId, selectionHitsJson())
        } else {
            MailNative.archiveMessages(activeAccountId, folderId, selectionUidsJson())
        }
        withContext(Dispatchers.Main) { offerUndo(result) }
        afterBulk()
    }

    fun bulkMove(destPath: String) = io {
        MailNative.ensureInit(appContext)
        val result = if (searchActive) {
            MailNative.moveHits(activeAccountId, selectionHitsJson(), destPath)
        } else {
            MailNative.moveMessages(activeAccountId, folderId, selectionUidsJson(), destPath)
        }
        withContext(Dispatchers.Main) { offerUndo(result) }
        afterBulk()
    }

    /** To Trash (undoable); the UI confirms first per the delete preference. */
    fun bulkTrash() = io {
        MailNative.ensureInit(appContext)
        val result = if (searchActive) {
            MailNative.deleteHits(activeAccountId, selectionHitsJson())
        } else {
            MailNative.deleteMessages(activeAccountId, folderId, selectionUidsJson())
        }
        withContext(Dispatchers.Main) { offerUndo(result) }
        afterBulk()
    }

    /** Destroy server-side. No undo — the UI always confirms first. */
    fun bulkPurge() = io {
        MailNative.ensureInit(appContext)
        if (searchActive) {
            MailNative.purgeHits(activeAccountId, selectionHitsJson())
        } else {
            MailNative.purgeMessages(activeAccountId, folderId, selectionUidsJson())
        }
        afterBulk()
    }

    private suspend fun afterBulk() {
        withContext(Dispatchers.Main) { exitSelectionMode() }
        reloadMessages()
        loadFolders()
        if (searchActive && similarLabel == null) runSearch()
    }

    fun selectAccount(id: Long) = io {
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
    }

    private fun loadFolders() = io {
        MailNative.ensureInit(appContext)
        val id = activeAccountId
        if (id < 0) {
            withContext(Dispatchers.Main) { folders = emptyList() }
            return@io
        }
        val tree = parseFolders(MailNative.foldersJson(id))
        withContext(Dispatchers.Main) {
            folders = tree
            if (folderId >= 0 && tree.none { it.id == folderId }) {
                folderId = -1
                messages = emptyList()
            }
        }
    }

    fun refreshFolders(andMessages: Boolean = false) {
        loadFolders()
        if (andMessages) reloadMessages()
        refreshOutbox()
    }

    /** Cache-first open: paint cached rows at once, fill from the server after. */
    fun openFolder(id: Long) {
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
        io {
            runCatching { MailNative.syncFolder(activeAccountId, id) }
                .onFailure { fail(it.message ?: "sync failed") }
        }
    }

    fun reloadMessages() = io {
        MailNative.ensureInit(appContext)
        val id = folderId
        if (id < 0) return@io
        val counts = JSONObject(MailNative.folderCounts(id))
        // Every cached row, like Qt/Flutter: mail fetched with "Load older"
        // stays on the list. A fixed page would hide each older batch behind
        // the newest 200 forever.
        val cached = counts.optInt("cached", PAGE.toInt()).coerceAtLeast(1).toLong()
        val rows = parseMessages(MailNative.messagesJson(id, cached, 0))
        withContext(Dispatchers.Main) {
            if (folderId == id) {
                messages = rows
                val older = counts.optString("older")
                val cached = counts.optInt("cached", rows.size)
                val server = counts.optInt("server", -1)
                canLoadOlder = counts.optBoolean("can_load_older", false)
                olderState = older
                olderLabel = when (older) {
                    "unchecked" -> "Cached $cached (server not checked)"
                    "partial" -> "Cached $cached of $server"
                    else -> "All $cached loaded"
                }
                recomputeShown()
            }
        }
    }

    fun loadMore() = io {
        MailNative.ensureInit(appContext)
        runCatching { MailNative.loadOlderMessages(activeAccountId, folderId) }
            .onFailure { fail(it.message ?: "load older failed") }
        // The finished event reloads the page; rows appear then.
    }

    fun syncNow() {
        val id = activeAccountId
        if (id < 0) {
            io { putStatus("No accounts yet", true) }
            return
        }
        syncAccount(id)
    }

    /**
     * Full sync of one account by id. A second pull while one is in flight is
     * ignored rather than failed: the running job's finished event still
     * clears the spinner, so the UI never sticks on an error while mail
     * keeps arriving underneath.
     *
     * The spinner is set only when a job was actually queued. Setting it
     * before the queue call sticks it on forever when the call is refused:
     * no job means no finish event, and a finish already on its way clears
     * the flag before it is even set.
     */
    fun syncAccount(id: Long) = io {
        MailNative.ensureInit(appContext)
        val queued = runCatching { MailNative.syncAccount(id) }
            .onFailure { e ->
                if (!e.message.orEmpty().contains("already running", ignoreCase = true)) {
                    fail(e.message ?: "sync failed")
                }
            }.isSuccess
        if (queued) withContext(Dispatchers.Main) { syncing = true }
    }

    /**
     * Pull-to-refresh inside a folder syncs just that folder: new mail,
     * flags and pending moves for what is showing. The account-wide
     * [syncAccount] stays on the Sync button and the folders pane.
     */
    fun syncFolder(id: Long = folderId) = io {
        MailNative.ensureInit(appContext)
        val account = activeAccountId
        if (account < 0 || id < 0) return@io
        // Set only when queued (see syncAccount): a refused pull must not
        // light the spinner with no job behind it.
        val queued = runCatching { MailNative.syncFolder(account, id) }
            .onFailure { e ->
                if (!e.message.orEmpty().contains("already running", ignoreCase = true)) {
                    fail(e.message ?: "sync failed")
                }
            }.isSuccess
        if (queued) withContext(Dispatchers.Main) { syncing = true }
    }

    /** Re-read the folder list from the server (LIST); the event reloads. */
    fun refreshFolderList() {
        val id = activeAccountId
        if (id < 0) return
        io {
            val error = runCatching { MailNative.refreshFolders(id) }.exceptionOrNull()
            if (error == null) {
                // Set only when queued (see syncAccount): no job, no spinner.
                withContext(Dispatchers.Main) { foldersBusy = true }
            } else if (!error.message.orEmpty().contains("already running", ignoreCase = true)) {
                withContext(Dispatchers.Main) { foldersBusy = false }
                fail(error.message ?: "refresh failed")
            }
        }
    }

    /** Hide or show a folder in the sidebar. Display-only. */
    fun setFolderSubscribed(id: Long, subscribed: Boolean) = io {
        MailNative.ensureInit(appContext)
        MailNative.setFolderSubscribed(id, subscribed)
        loadFolders()
    }

    /**
     * Create [path] on the server (`/` nests; the core maps it onto the
     * account delimiter). [onDone] gets (ok, status) when the job finishes,
     * or straight away when it could not be queued.
     */
    fun createFolder(path: String, onDone: (Boolean, String) -> Unit) {
        val id = activeAccountId
        if (id < 0) {
            onDone(false, "No account")
            return
        }
        foldersBusy = true
        finishWaiters.getOrPut("Folders") { mutableListOf() }.add(onDone)
        io {
            runCatching { MailNative.createFolder(id, path) }
                .onFailure { e ->
                    withContext(Dispatchers.Main) {
                        foldersBusy = false
                        finishWaiters["Folders"]?.remove(onDone)
                        onDone(false, e.message ?: "Could not create the folder")
                    }
                }
        }
    }

    /** Move one message of [fromFolder] to [destPath]; offers undo. */
    fun moveMessage(fromFolder: Long, uid: Int, destPath: String) = io {
        MailNative.ensureInit(appContext)
        val result = MailNative.moveMessages(activeAccountId, fromFolder, "[$uid]", destPath)
        withContext(Dispatchers.Main) {
            offerUndo(result)
            if (searchActive && similarLabel == null) runSearch()
        }
        reloadMessages()
        loadFolders()
    }

    fun setSearch(query: String) {
        exitSelectionMode()
        serverSearchFired = false
        serverSearchPending = false
        searchQuery = query
        runSearch()
    }

    fun toggleSearchScope() {
        searchFolderOnly = !searchFolderOnly
        runSearch()
    }

    fun clearSearch() {
        searchJob?.cancel()
        exitSelectionMode()
        serverSearchFired = false
        serverSearchPending = false
        searchQuery = ""
        searchHits = emptyList()
        searchActive = false
        similarLabel = null
    }

    /** Mail like this one across the account, shown as list hits. */
    fun findSimilar(folder: Long, uid: Int) = io {
        val accountId = activeAccountId
        val hits = parseMessages(MailNative.similarJson(accountId, folder, uid))
        val subject = runCatching { MailNative.similarSubject(accountId, folder, uid) }.getOrDefault("")
        withContext(Dispatchers.Main) {
            searchJob?.cancel()
            exitSelectionMode()
            searchQuery = ""
            searchHits = hits
            searchActive = true
            shownHits = hits.filter { rowShown(it) }
            similarLabel = "Similar to: ${subject.ifEmpty { "this message" }}"
        }
    }

    fun loadReaderPrefs() = io {
        MailNative.ensureInit(appContext)
        val o = JSONObject(MailNative.settingsJson())
        val prefs = ReaderPrefs(
            autoMarkRead = o.optBoolean("auto_mark_read", true),
            markReadDelaySecs = o.optLong("mark_read_delay_secs", 0),
            loadRemoteImages = o.optBoolean("load_remote_images", false),
            confirmDelete = o.optBoolean("confirm_delete", true),
            linkClickAction = o.optString("link_click_action", "examine"),
            scale = when (o.optString("reader_font_size")) {
                "small" -> 12f / 14f
                "large" -> 18f / 14f
                else -> 1f
            },
        )
        withContext(Dispatchers.Main) { readerPrefs = prefs }
    }

    /** After the reader changed a message: re-read list, tree and search. */
    fun afterReaderChange() {
        reloadMessages()
        loadFolders()
        if (searchActive && similarLabel == null) runSearch()
    }

    private fun runSearch() {
        searchJob?.cancel()
        similarLabel = null
        val query = searchQuery
        val accountId = activeAccountId
        val folderScope = if (searchFolderOnly) openFolder?.path.orEmpty() else ""
        val shown = messages
        val shownFolder = folderId
        searchJob = scope.launch(Dispatchers.IO) {
            MailNative.ensureInit(appContext)
            val plan = runCatching { JSONObject(MailNative.searchPlan(query)) }.getOrNull()
            val rows = runCatching { when (plan?.optString("mode")) {
                // Short input filters the shown folder's rows in place.
                "filter" -> shown
                    .filter {
                        MailNative.searchFilterMatches(
                            plan.optString("query"), it.subject, it.from, it.fromName, it.snippet,
                        ) == "true"
                    }
                    .map { it.copy(folderId = shownFolder) }
                "indexed" -> {
                    delay(plan.optLong("debounce_ms", 0))
                    val found = parseMessages(MailNative.searchJson(accountId, plan.optString("query"), folderScope))
                    // Thin index results trigger one queued server backfill
                    // per query; its finish event re-runs this search.
                    if (!serverSearchFired && found.size < plan.optInt("hit_limit", 50)) {
                        serverSearchFired = true
                        delay(plan.optLong("debounce_ms", 0))
                        if (searchQuery == query) {
                            runCatching {
                                MailNative.searchServer(accountId, plan.optString("query"), folderScope)
                            }.onSuccess {
                                scope.launch(Dispatchers.Main) { serverSearchPending = true }
                            }
                        }
                    }
                    found
                }
                else -> null
            } }.getOrElse { if (it is kotlinx.coroutines.CancellationException) throw it else emptyList() }
            withContext(Dispatchers.Main) {
                if (searchQuery != query) return@withContext
                searchActive = rows != null
                searchHits = rows.orEmpty()
                shownHits = rows.orEmpty().filter { rowShown(it) }
                pruneSelection()
            }
        }
    }

    fun undo() = io {
        val batch = undoOffer?.batch
        withContext(Dispatchers.Main) { undoOffer = null }
        if (batch.isNullOrEmpty()) return@io
        MailNative.ensureInit(appContext)
        val text = runCatching { MailNative.undoMove(batch) }
            .getOrElse { it.message ?: "undo failed" }
        putStatus(text, false)
        reloadMessages()
        // The rows are visible again: the tree counts must come back too,
        // like the Flutter undo does (messages + folders + hits).
        loadFolders()
        if (searchActive && similarLabel == null) runSearch()
    }

    fun offerUndo(resultJson: String) {
        val o = runCatching { JSONObject(resultJson) }.getOrNull() ?: return
        val batch = o.optString("batch")
        if (batch.isEmpty()) return
        undoOffer = UndoOffer(batch, o.optString("label"))
    }

    /** Delete with folders, messages and secrets; land on the next account. */
    fun removeAccount(id: Long) = io {
        MailNative.ensureInit(appContext)
        runCatching { MailNative.deleteAccount(id) }
            .onFailure { fail(it.message ?: "delete failed") }
        refreshAll()
    }

    fun markSeen() = io {
        runCatching {
            MailNative.ensureInit(appContext)
            MailNative.backgroundMarkSeen()
        }
    }

    fun refreshOutbox() = io {
        MailNative.ensureInit(appContext)
        val id = activeAccountId
        if (id < 0) {
            withContext(Dispatchers.Main) {
                outboxPending = 0
                outboxFailed = false
            }
            return@io
        }
        val o = JSONObject(MailNative.outboxStatusJson(id))
        withContext(Dispatchers.Main) {
            outboxPending = o.optInt("pending", 0)
            outboxFailed = o.optInt("failed", 0) > 0
        }
    }

    companion object {
        const val PAGE = 200L

        fun parseAccounts(json: String): List<Account> {
            val arr = runCatching { org.json.JSONArray(json) }.getOrElse { return emptyList() }
            return List(arr.length()) { i ->
                val o = arr.optJSONObject(i) ?: JSONObject()
                Account(
                    id = o.optLong("id", -1),
                    email = o.optString("email"),
                    name = o.optString("name").ifEmpty { o.optString("email") },
                    initials = o.optString("initials", "?"),
                    avatarLight = o.optString("avatar_light"),
                    avatarDark = o.optString("avatar_dark"),
                )
            }.filter { it.id >= 0 }
        }

        fun parseFolders(json: String): List<Folder> {
            val arr = runCatching { org.json.JSONArray(json) }.getOrElse { return emptyList() }
            return List(arr.length()) { i ->
                val o = arr.optJSONObject(i) ?: JSONObject()
                val path = o.optString("name")
                Folder(
                    id = o.optLong("id", -1),
                    path = path,
                    leaf = o.optString("leaf").ifEmpty { path },
                    depth = o.optInt("depth", 0),
                    role = o.optString("role"),
                    unread = o.optInt("unread", 0),
                    count = o.optInt("count", 0),
                    subscribed = o.optBoolean("subscribed", true),
                    deleteIsPermanent = o.optBoolean("delete_is_permanent", false),
                )
            }.filter { it.id >= 0 }
        }

        fun parseMessages(json: String): List<MessageRow> {
            val arr = runCatching { org.json.JSONArray(json) }.getOrElse { return emptyList() }
            return List(arr.length()) { i ->
                val o = arr.optJSONObject(i) ?: JSONObject()
                MessageRow(
                    uid = o.optInt("uid", -1),
                    subject = o.optString("subject", "(no subject)"),
                    from = o.optString("from"),
                    fromName = o.optString("from_name"),
                    date = o.optString("date"),
                    snippet = o.optString("snippet"),
                    unread = o.optBoolean("unread", false),
                    starred = o.optBoolean("starred", false),
                    hasAttachments = o.optBoolean("has_attachments", false),
                    dateRaw = o.optString("date_raw"),
                    initials = o.optString("initials", "?"),
                    avatarLight = o.optString("avatar_light"),
                    avatarDark = o.optString("avatar_dark"),
                    folderId = o.optLong("folder_id", -1),
                )
            }.filter { it.uid >= 0 }
        }
    }
}
