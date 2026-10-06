package de.renier.mailclient.ui.state

import android.content.Context
import android.os.SystemClock
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import de.renier.mailclient.JobEvents
import de.renier.mailclient.MailNative
import de.renier.mailclient.MailSchedule
import de.renier.mailclient.ui.composer.ComposerSeed
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
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
    // The display name sent in From (the composer prefills it).
    val fromName: String = "",
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
    // Collapse rule from the feed (`FolderRole::always_visible`): known
    // folders stay visible inside a collapsed parent; only custom
    // subfolders fold away.
    val alwaysVisible: Boolean = true,
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

/** A composition sent but not yet accepted by SMTP, as the composer held it. */
data class PendingSend(val seed: ComposerSeed, val accountId: Long, val folderId: Long)

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
    // Job kinds queued or running on the net thread ("Sync", "Folders",
    // "Search", …), mirrored from the core's own in-flight table: every job
    // event carries it, newest generation wins. Never set by hand — a
    // Kotlin-side flag drifts the moment a job is refused, deduped, queued
    // from another screen or outlives the composition that started it.
    var busyKinds: Set<String> by mutableStateOf(emptySet())
        private set
    private var busyGeneration = -1L
    val busy: Boolean get() = busyKinds.isNotEmpty()
    val syncing: Boolean get() = "Sync" in busyKinds
    var outboxPending by mutableStateOf(0)
        private set
    var outboxFailed by mutableStateOf(false)
        private set
    // The chip's words from the core ("2 unsent (1 failed)") and how many
    // rows a sync could still deliver.
    var outboxLabel by mutableStateOf("")
        private set
    var outboxRetryable by mutableStateOf(0)
        private set
    var undoOffer: UndoOffer? by mutableStateOf(null)
        private set
    var notice: String? by mutableStateOf(null)
        private set
    // Some account checks in the background (push or poll, quiet or not):
    // the shell then asks for the notification permission.
    var backgroundChecks by mutableStateOf(false)
        private set
    // Interface scale (ui_scale) and list density, applied by the shell.
    var uiScale by mutableStateOf(1f)
        private set
    var compactList by mutableStateOf(false)
        private set
    // Server capabilities per account, from the "Capabilities" job (About).
    var capabilities by mutableStateOf<Map<Long, JSONObject>>(emptyMap())
        private set
    // A "Folders" job (LIST refresh, create) is queued or running.
    val foldersBusy: Boolean get() = "Folders" in busyKinds

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

    // Qt's sendPending: the composer closes as soon as the send is queued,
    // and a failure before SMTP accepts reopens it with the text. The core
    // has dropped the MIME by then, so the retry is the user's and cannot
    // send twice. Set off the main thread just before the job starts.
    @Volatile
    var pendingSend: PendingSend? = null

    /** A failed send for the shell to reopen. */
    var reopenSend by mutableStateOf<PendingSend?>(null)

    fun consumeReopenSend() {
        reopenSend = null
    }

    private fun settleSend(phase: String, ok: Boolean, status: String) {
        val p = pendingSend ?: return
        // A Send job reports progress once, at SMTP acceptance, and fails only
        // before it (later trouble is "sent, but…"). So an earlier send still
        // filing its Sent copy can never settle this one by mistake.
        when {
            phase == "progress" -> pendingSend = null
            phase == "finished" && !ok -> {
                pendingSend = null
                reopenSend = p.copy(seed = p.seed.copy(failure = status.ifEmpty { "Sending failed" }))
            }
        }
    }

    fun info(msg: String) {
        notice = msg
    }

    fun consumeNotice() {
        notice = null
    }

    /** [offer]'s bar is gone; a newer offer stays. */
    fun dismissUndo(offer: UndoOffer) {
        if (undoOffer === offer) undoOffer = null
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
        }
    }

    /** Main thread. Drops snapshots older than one already applied. */
    private fun applyBusy(o: JSONObject?) {
        if (o == null) return
        val generation = o.optLong("generation", -1)
        if (generation < busyGeneration) return
        busyGeneration = generation
        val arr = o.optJSONArray("kinds")
        busyKinds = if (arr == null) emptySet() else List(arr.length()) { arr.optString(it) }.toSet()
    }

    private fun loadBusy() = io {
        MailNative.ensureInit(appContext)
        val o = runCatching { JSONObject(MailNative.netBusy()) }.getOrNull()
        withContext(Dispatchers.Main) { applyBusy(o) }
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
        // Jobs may already run (started before this composition): pick up
        // the core's table instead of assuming idle.
        loadBusy()
        // Cold start: the cache paints first, then the account syncs, like
        // Flutter's start() — a slow server never holds the first paint.
        refreshAll(syncAfter = true)
        loadReaderPrefs()
    }

    fun release() {
        jobEvents?.close()
        jobEvents = null
        autoSyncJob?.cancel()
    }

    // ---- Foreground auto-sync (Flutter's start/resumed/_autoSyncTick) ----

    // elapsedRealtime of the last account sync asked for; 0 = never.
    @Volatile private var lastSyncRequest = 0L
    private var autoSyncMinutes = 0
    private var autoSyncJob: Job? = null
    private var foreground = true

    /**
     * Back in the foreground. Android froze the timer meanwhile, while the
     * background check may have filled the cache: show the cache at once,
     * then sync unless auto-sync is off or a sync was asked for within the
     * last minute (a quick app switch, the startup sync).
     */
    fun resumed() {
        foreground = true
        refreshFolders(andMessages = true)
        markSeen()
        io {
            val id = activeAccountId
            if (id < 0) return@io
            loadAutoSyncMinutes(id)
            val gapOk = lastSyncRequest == 0L ||
                SystemClock.elapsedRealtime() - lastSyncRequest >= RESUME_SYNC_GAP_MS
            withContext(Dispatchers.Main) {
                restartAutoSync()
                if (autoSyncMinutes > 0 && !syncing && gapOk) syncAccount(id)
            }
        }
    }

    /**
     * Re-plan the background checks from every account's settings and run
     * the plan (push service, poller, quiet-hours replan). Database only.
     * Called on start and after account changes; boot, app updates and the
     * clock re-plan on their own (MailSchedule).
     */
    fun rescheduleBackground() = io {
        val plan = MailSchedule.refresh(appContext)
        withContext(Dispatchers.Main) { backgroundChecks = plan?.optBoolean("any") == true }
    }

    /** Left the foreground: no timer ticks while nobody is looking. */
    fun paused() {
        // Mail synced while the app was open was on screen: the background
        // check must not alert for it later.
        markSeen()
        foreground = false
        autoSyncJob?.cancel()
        autoSyncJob = null
    }

    /** The open account's effective interval (its override or the app's). */
    private suspend fun loadAutoSyncMinutes(accountId: Long) {
        val minutes = runCatching {
            JSONObject(MailNative.accountSettingsJson(accountId))
                .optJSONObject("effective")
                ?.optString("sync_interval_minutes")
                ?.toIntOrNull()
        }.getOrNull() ?: 0
        withContext(Dispatchers.Main) { autoSyncMinutes = minutes }
    }

    /** Main thread. One timer per state, only while in the foreground. */
    private fun restartAutoSync() {
        autoSyncJob?.cancel()
        autoSyncJob = null
        val minutes = autoSyncMinutes
        if (minutes <= 0 || !foreground) return
        autoSyncJob = scope.launch(Dispatchers.Main) {
            while (true) {
                delay(minutes * 60_000L)
                val id = activeAccountId
                if (id >= 0 && !syncing) syncAccount(id)
            }
        }
    }


    private fun onJobEvent(json: String) {
        val e = runCatching { JSONObject(json) }.getOrNull()
        if (e == null) {
            Log.w(TAG, "dropping unparsable job event")
            return
        }
        val kind = e.optString("kind")
        val wasBusy = busy
        applyBusy(e.optJSONObject("busy"))
        val phase = e.optString("phase")
        if (kind == "Send") settleSend(phase, e.optBoolean("ok", true), e.optString("status"))
        if (phase == "queued") {
            // Something to read before the job's first progress arrives; a
            // job queued behind a running one leaves that one's status alone.
            if (!wasBusy) {
                status = when (kind) {
                    "Sync" -> "Syncing…"
                    "Folders" -> "Updating folders…"
                    "Search" -> "Searching the server…"
                    "Send" -> "Sending…"
                    else -> "$kind…"
                }
                statusError = false
            }
            return
        }
        val ok = e.optBoolean("ok", true)
        val text = e.optString("status")
        if (kind == "Capabilities" && ok && phase == "finished") {
            val caps = runCatching { JSONObject(text) }.getOrNull()
            if (caps != null) {
                capabilities = capabilities + (caps.optLong("account_id", -1) to caps)
                status = "Server capabilities loaded"
                statusError = false
            }
        } else if (text.isNotEmpty()) {
            status = text
            statusError = !ok
        }
        if (phase != "finished") return
        Log.d(TAG, "job finished: kind=$kind ok=$ok busy=$busyKinds")
        // A finished server backfill lands via the re-run below.
        if (kind == "Search") serverSearchPending = false
        finishWaiters.remove(kind)?.forEach { it(ok, e.optString("status")) }
        val accountId = e.optLong("account_id", -1)
        val eventFolder = e.optLong("folder_id", -1)
        // Re-read whatever is showing, like the Dart side does: the folder
        // tree (counts and unread pills) on every finished job for the
        // account — a folder sync or load-older moves them too — plus the
        // messages when the job touched the open folder or every folder.
        if (kind == "Folders" || accountId == activeAccountId) {
            loadFolders()
            if (kind == "Folders" || eventFolder == -1L || eventFolder == folderId) {
                reloadMessages()
            }
        }
        refreshOutbox()
        if (searchActive && similarLabel == null) runSearch()
    }

    fun refreshAll(syncAfter: Boolean = false) = io {
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

    // One row's actions (the list's ⋮ menu). A search hit acts in the
    // folder it lives in, a list row in the open one.

    /** The folder [m] lives in. */
    fun rowFolderId(m: MessageRow): Long = rowFolder(m)?.id ?: folderId

    /** Delete in [m]'s folder destroys instead of moving to Trash. */
    fun rowDeleteIsPermanent(m: MessageRow): Boolean = rowFolder(m)?.deleteIsPermanent == true

    fun rowMarkRead(m: MessageRow, read: Boolean) = io {
        MailNative.ensureInit(appContext)
        MailNative.markReadMany(activeAccountId, rowFolderId(m), "[${m.uid}]", read)
        afterRow()
    }

    fun rowStar(m: MessageRow, starred: Boolean) = io {
        MailNative.ensureInit(appContext)
        MailNative.setStarMany(activeAccountId, rowFolderId(m), "[${m.uid}]", starred)
        afterRow()
    }

    fun rowArchive(m: MessageRow) = io {
        MailNative.ensureInit(appContext)
        val result = MailNative.archiveMessages(activeAccountId, rowFolderId(m), "[${m.uid}]")
        withContext(Dispatchers.Main) { offerUndo(result) }
        afterRow()
    }

    fun rowMove(m: MessageRow, destPath: String) = io {
        MailNative.ensureInit(appContext)
        val result = MailNative.moveMessages(activeAccountId, rowFolderId(m), "[${m.uid}]", destPath)
        withContext(Dispatchers.Main) { offerUndo(result) }
        afterRow()
    }

    /** To Trash (undoable), or destroyed where the folder says so. */
    fun rowTrash(m: MessageRow) = io {
        MailNative.ensureInit(appContext)
        val result = MailNative.deleteMessages(activeAccountId, rowFolderId(m), "[${m.uid}]")
        withContext(Dispatchers.Main) { offerUndo(result) }
        afterRow()
    }

    /** Destroy server-side. No undo — the UI always confirms first. */
    fun rowPurge(m: MessageRow) = io {
        MailNative.ensureInit(appContext)
        MailNative.purgeMessages(activeAccountId, rowFolderId(m), "[${m.uid}]")
        afterRow()
    }

    private suspend fun afterRow() {
        reloadMessages()
        loadFolders()
        withContext(Dispatchers.Main) { if (searchActive && similarLabel == null) runSearch() }
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
        // A switch syncs the account it lands on, like Flutter; its own
        // interval replaces the previous account's timer.
        val landed = activeAccountId
        if (landed >= 0) {
            loadAutoSyncMinutes(landed)
            withContext(Dispatchers.Main) { restartAutoSync() }
            syncAccount(landed)
        }
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
                .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "sync failed") }
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
            .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "load older failed") }
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
     * Full sync of one account by id. Only queues: the busy line comes from
     * the core's in-flight table (see [busyKinds]). A second pull while the
     * same job runs is refused by the core's dedupe and ignored here — the
     * line already shows it.
     */
    fun syncAccount(id: Long) = io {
        MailNative.ensureInit(appContext)
        lastSyncRequest = SystemClock.elapsedRealtime()
        runCatching { MailNative.syncAccount(id) }
            .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "sync failed") }
    }

    /**
     * Pull-to-refresh inside a folder syncs just that folder: new mail,
     * flags and pending moves for what is showing. The account-wide
     * [syncAccount] stays on the Sync entry and the folders pane.
     */
    fun syncFolder(id: Long = folderId) = io {
        MailNative.ensureInit(appContext)
        val account = activeAccountId
        if (account < 0 || id < 0) return@io
        runCatching { MailNative.syncFolder(account, id) }
            .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "sync failed") }
    }

    /** Re-read the folder list from the server (LIST); the event reloads. */
    fun refreshFolderList() {
        val id = activeAccountId
        if (id < 0) return
        io {
            runCatching { MailNative.refreshFolders(id) }
                .onFailure { if (!it.isAlreadyRunning()) fail(it.message ?: "refresh failed") }
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

    /**
     * Suspend until the next finished event of [kind], queuing via [queue]
     * first. The waiter is registered on the main thread *before* [queue]
     * runs, so a fast job cannot finish unseen — the same ordering
     * Flutter's attachment download relies on. [queue] throwing skips the
     * wait (a refused queue has no finish event coming); a timeout answers
     * null and the caller re-reads the cache. Cancellation drops the waiter.
     */
    suspend fun awaitFinished(
        kind: String,
        timeoutMs: Long = 120_000,
        queue: () -> Unit,
    ): Pair<Boolean, String>? = withContext(Dispatchers.Main) {
        val waiter = CompletableDeferred<Pair<Boolean, String>>()
        val cb: (Boolean, String) -> Unit = { ok, status -> waiter.complete(ok to status) }
        finishWaiters.getOrPut(kind) { mutableListOf() }.add(cb)
        try {
            queue()
            withTimeoutOrNull(timeoutMs) { waiter.await() }
        } finally {
            finishWaiters[kind]?.remove(cb)
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
        val scale = o.optDouble("ui_scale", 1.0).toFloat().takeIf { it in 0.5f..3f } ?: 1f
        val compact = o.optString("list_density") == "compact"
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
        withContext(Dispatchers.Main) {
            readerPrefs = prefs
            uiScale = scale
            compactList = compact
        }
    }

    /**
     * Settings were saved: re-read everything that acts on them — reader
     * and list preferences, sort, the open account's auto-sync interval —
     * and re-plan the background checks.
     */
    fun settingsSaved() {
        loadReaderPrefs()
        loadSort()
        reloadMessages()
        rescheduleBackground()
        val id = activeAccountId
        if (id >= 0) {
            io {
                loadAutoSyncMinutes(id)
                withContext(Dispatchers.Main) { restartAutoSync() }
            }
        }
    }

    /** Ask the server for its capability list; arrives in [capabilities]. */
    fun refreshCapabilities(accountId: Long) = io {
        MailNative.ensureInit(appContext)
        try {
            MailNative.refreshServerCapabilities(accountId)
        } catch (e: Exception) {
            if (!e.isAlreadyRunning()) fail(e.message ?: "Could not ask the server")
        }
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

    /** Seconds an offer stays undoable (the core's grace period). */
    val undoGraceSecs: Long by lazy { runCatching { MailNative.undoGraceSecs().toLong() }.getOrDefault(10L) }

    /** Take back the current offer (the snackbar's Undo, Ctrl+Z). Main thread. */
    fun undo() {
        val batch = undoOffer?.batch
        undoOffer = null
        if (!batch.isNullOrEmpty()) undoBatch(batch)
    }

    private fun undoBatch(batch: String) = io {
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
                outboxLabel = ""
                outboxRetryable = 0
            }
            return@io
        }
        val o = JSONObject(MailNative.outboxStatusJson(id))
        withContext(Dispatchers.Main) {
            outboxPending = o.optInt("pending", 0)
            outboxFailed = o.optInt("failed", 0) > 0
            outboxLabel = o.optString("label")
            outboxRetryable = o.optInt("retryable", 0)
        }
    }

    companion object {
        const val PAGE = 200L
        // Flutter's shouldSyncOnResume gap.
        const val RESUME_SYNC_GAP_MS = 60_000L
        const val TAG = "MailState"

        fun parseAccounts(json: String): List<Account> {
            val arr = runCatching { org.json.JSONArray(json) }.getOrElse { return emptyList() }
            return List(arr.length()) { i ->
                val o = arr.optJSONObject(i) ?: JSONObject()
                Account(
                    id = o.optLong("id", -1),
                    email = o.optString("email"),
                    name = o.optString("name").ifEmpty { o.optString("email") },
                    fromName = o.optString("from_name").takeIf { it != "null" }.orEmpty(),
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
                val role = o.optString("role")
                Folder(
                    id = o.optLong("id", -1),
                    path = path,
                    leaf = o.optString("leaf").ifEmpty { path },
                    depth = o.optInt("depth", 0),
                    role = role,
                    unread = o.optInt("unread", 0),
                    count = o.optInt("count", 0),
                    subscribed = o.optBoolean("subscribed", true),
                    alwaysVisible = o.optBoolean("always_visible", role != "custom"),
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

/** A queue call refused because the same job is in flight: not an error. */
internal fun Throwable.isAlreadyRunning(): Boolean =
    message.orEmpty().contains("already running", ignoreCase = true)
