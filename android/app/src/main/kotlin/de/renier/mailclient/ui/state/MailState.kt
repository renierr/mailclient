package de.renier.mailclient.ui.state

import android.content.Context
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.mutableStateSetOf
import androidx.compose.runtime.setValue
import de.renier.mailclient.JobEvents
import de.renier.mailclient.MailNative
import de.renier.mailclient.MailSchedule
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicBoolean

// Shell state: what the app shows and what it does, over the JNI surface.
// Plain holder (no new dependencies — no ViewModel, no
// navigation-compose), owned by MailShell's composition. Reads take explicit
// ids and jobs only say *that* something changed, so this re-reads whatever
// is showing — the same contract the Dart MailState keeps.
//
// Behaviour lives in sibling files by responsibility (same package, pure
// move — call syntax is unchanged):
//   MailModels.kt   — Account/Folder/ReaderPrefs/MessageRow/… data classes
//   MailParsers.kt  — feed JSON decoders
//   MailStateFolders.kt — accounts, folders, message pages, sync queueing
//   MailStateList.kt    — sort, filters, selection, bulk / row actions
//   MailStateSearch.kt  — search, find-similar, filtered views
//   MailStatePrefs.kt   — reader prefs, capabilities, outbox, undo, removal
class MailState(internal val appContext: Context, internal val scope: CoroutineScope) {
    // Every UI-read field is snapshot state: plain vars never recompose.
    // All writes already hop to Dispatchers.Main (see io/putStatus).
    var initialized by mutableStateOf(false)
        internal set
    private var jobEvents: AutoCloseable? = null
    var accounts: List<Account> by mutableStateOf(emptyList())
        internal set
    var activeAccountId by mutableStateOf(-1L)
        internal set
    var folders: List<Folder> by mutableStateOf(emptyList())
        internal set
    var folderId by mutableStateOf(-1L)
        internal set
    // Expanded folder parents, by id. Lives here (not in the pane) so
    // navigating into a folder and back keeps the tree as it was.
    // In-memory: every launch starts collapsed (default closed). Mutate
    // only through toggleFolderExpanded.
    val expandedFolders = mutableStateSetOf<Long>()
    // Painted sidebar rows for the current expanded set, folded by the core
    // (`mailcore::feed::sidebar_rows`). Re-folded on every folders load and
    // toggle; the sidebar paints these joined to `folders` by id.
    var sidebarRows: List<SidebarRow> by mutableStateOf(emptyList())
        internal set
    // Bumped by each refreshSidebarRows (main thread only): its result
    // paints only while it is still the latest request.
    internal var sidebarGeneration = 0
    var messages: List<MessageRow> by mutableStateOf(emptyList())
        internal set
    var canLoadOlder by mutableStateOf(false)
        internal set
    // The load-older footer's raw state and its words from the core
    // (`feed::older_label`); hidden when the server holds nothing
    // ("empty"), like the desktop footer.
    var olderState by mutableStateOf("")
        internal set
    var olderLabel by mutableStateOf("")
        internal set
    // Cached rows and the server's count (-1: never reported) behind it.
    internal var olderCounts = 0 to -1
    var status by mutableStateOf("Starting…")
        internal set
    var statusError by mutableStateOf(false)
        internal set
    // Job kinds queued or running on the net thread ("Sync", "Folders",
    // "Search", …), mirrored from the core's own in-flight table: every job
    // event carries it, newest generation wins. Never set by hand — a
    // Kotlin-side flag drifts the moment a job is refused, deduped, queued
    // from another screen or outlives the composition that started it.
    var busyKinds: Set<String> by mutableStateOf(emptySet())
        internal set
    private var busyGeneration = -1L
    val busy: Boolean get() = busyKinds.isNotEmpty()
    val syncing: Boolean get() = "Sync" in busyKinds
    var outboxPending by mutableStateOf(0)
        internal set
    var outboxFailed by mutableStateOf(false)
        internal set
    // The chip's words from the core ("2 unsent (1 failed)") and how many
    // rows a sync could still deliver.
    var outboxLabel by mutableStateOf("")
        internal set
    var outboxRetryable by mutableStateOf(0)
        internal set
    var undoOffer: UndoOffer? by mutableStateOf(null)
        internal set
    var notice: String? by mutableStateOf(null)
        internal set
    // Some account checks in the background (push or poll, quiet or not):
    // the shell then asks for the notification permission.
    var backgroundChecks by mutableStateOf(false)
        internal set
    // Interface scale (ui_scale) and list density, applied by the shell.
    var uiScale by mutableStateOf(1f)
        internal set
    var compactList by mutableStateOf(false)
        internal set
    // Server capabilities per account, from the "Capabilities" job (About).
    var capabilities by mutableStateOf<Map<Long, JSONObject>>(emptyMap())
        internal set
    // Why the last refresh failed, per account; kept beside an older list.
    var capabilitiesError by mutableStateOf<Map<Long, String>>(emptyMap())
        internal set
    // The account last asked: a failed job's event names no account.
    @Volatile
    internal var capabilitiesFor = -1L
    // A "Folders" job (LIST refresh, create) is queued or running.
    val foldersBusy: Boolean get() = "Folders" in busyKinds

    // Search: the typed text, whether it is scoped to the open folder, and
    // the rows it found. How a query runs (off / row filter / FTS index,
    // debounce) is the core's call via searchPlan.
    var searchQuery by mutableStateOf("")
        internal set
    var searchFolderOnly by mutableStateOf(false)
        internal set
    var searchHits: List<MessageRow> by mutableStateOf(emptyList())
        internal set
    var searchActive by mutableStateOf(false)
        internal set
    // "Similar to: …" when the hits are a find-similar result, not a query.
    var similarLabel: String? by mutableStateOf(null)
        internal set
    // The message the similar hits are about, so they refresh like Qt's.
    internal var similarTarget: Pair<Long, Int>? = null
    // A server backfill is in flight for the current query (thin index
    // results, like Qt/Flutter); the Search job's finish re-runs the query.
    var serverSearchPending by mutableStateOf(false)
        internal set
    // Set from the search coroutine (IO), reset on Main.
    internal val serverSearchFired = AtomicBoolean(false)
    // 1–2 letters (core plan "filter"): the open folder's rows filtered in
    // place, like Qt — still the folder view (count, sort, footer), not a
    // search. "" when no row filter is on.
    var rowFilterQuery by mutableStateOf("")
        internal set
    // Step 4 list state: sort (persisted in core settings), AND-combined
    // quick filters, and multi-select. The core sorts the page itself; the
    // filters apply client-side to loaded rows and search hits alike, like
    // the Flutter list — they never fetch.
    var sortField by mutableStateOf("date")
        internal set
    var sortDesc by mutableStateOf(true)
        internal set
    var filterUnread by mutableStateOf(false)
        internal set
    var filterStarred by mutableStateOf(false)
        internal set
    var filterAttachments by mutableStateOf(false)
        internal set
    // YYYY-MM-DD day bounds, After inclusive / Before exclusive, "" = unset.
    var filterAfter by mutableStateOf("")
        internal set
    var filterBefore by mutableStateOf("")
        internal set
    // Words for the active date filter ("Today", "Mar 3 – Mar 9", …).
    var dateFilterLabel by mutableStateOf("")
        internal set
    var selectionMode by mutableStateOf(false)
        internal set
    // Folder rows key by uid; search hits span folders, keyed folderId:uid.
    var selectedKeys: Set<String> by mutableStateOf(emptySet())
        internal set
    // Loaded rows minus the quick filters; what the list actually paints.
    var shownMessages: List<MessageRow> by mutableStateOf(emptyList())
        internal set
    var shownHits: List<MessageRow> by mutableStateOf(emptyList())
        internal set
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
        internal set
    internal var searchJob: Job? = null

    // One-shot callbacks for the next finished event of a job kind, keyed by
    // kind. Main thread only (registered and drained there).
    internal val finishWaiters = mutableMapOf<String, MutableList<(Boolean, String) -> Unit>>()

    // Qt's sendPending: the composer closes as soon as the send is queued,
    // and a failure before SMTP accepts reopens it with the text. The core
    // has dropped the MIME by then, so the retry is the user's and cannot
    // send twice. Set off the main thread just before the job starts.
    @Volatile
    var pendingSend: PendingSend? = null

    // The same for Save draft: the composer closes once the save is queued,
    // and a failed save reopens it with the text (Qt keeps it open until
    // the save lands; either way nothing typed is lost).
    @Volatile
    var pendingDraft: PendingSend? = null

    /** A failed send for the shell to reopen. */
    var reopenSend by mutableStateOf<PendingSend?>(null)
        private set

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
                reopenSend = p.copy(seed = p.seed.copy(failure = "Not sent: ${status.ifEmpty { "Sending failed" }}"))
            }
        }
    }

    private fun settleDraft(phase: String, ok: Boolean, status: String) {
        val p = pendingDraft ?: return
        if (phase != "finished") return
        pendingDraft = null
        if (!ok) {
            reopenSend = p.copy(seed = p.seed.copy(failure = "Draft not saved: ${status.ifEmpty { "saving failed" }}"))
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

    internal fun io(work: suspend () -> Unit) {
        scope.launch(Dispatchers.IO) {
            try {
                work()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                fail(e.message ?: "failed")
            }
        }
    }

    internal suspend fun fail(msg: String) {
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

    internal suspend fun putStatus(text: String, error: Boolean) {
        withContext(Dispatchers.Main) {
            status = text
            statusError = error
        }
    }

    /** First load + job-event listener. Idempotent for the composition. */
    fun ensureInit() {
        if (initialized) {
            subscribeJobs()
            refreshAll()
            return
        }
        initialized = true
        scope.launch {
            // Opening the database can run a schema migration after an
            // update: never on the main thread. MailApplication started it
            // already, so this usually only waits for that to finish. The
            // job subscription waits too: registering it opens the core.
            withContext(Dispatchers.IO) { MailNative.ensureInit(appContext) }
            subscribeJobs()
            // Jobs may already run (started before this composition): pick up
            // the core's table instead of assuming idle.
            loadBusy()
            // Cold start: the cache paints first, then the account syncs, like
            // Flutter's start() — a slow server never holds the first paint.
            refreshAll(syncAfter = true, coldStart = true)
            loadReaderPrefs()
        }
    }

    private fun subscribeJobs() {
        if (jobEvents != null) return
        jobEvents =
            JobEvents.subscribe(appContext) { json ->
                scope.launch(Dispatchers.Main) { onJobEvent(json) }
            }
    }

    fun release() {
        jobEvents?.close()
        jobEvents = null
        autoSyncJob?.cancel()
    }

    // ---- Foreground auto-sync (Flutter's start/resumed/_autoSyncTick) ----

    private var autoSyncMinutes = 0
    private var autoSyncJob: Job? = null
    private var foreground = true

    /**
     * Back in the foreground. Android froze the timer meanwhile, while the
     * background check may have filled the cache: show the cache at once,
     * then sync unless auto-sync is off, one is running, or the account
     * finished one within the core's grace period (a quick app switch, the
     * startup sync, a background check). Only this trigger is held back.
     */
    fun resumed() {
        foreground = true
        refreshFolders(andMessages = true)
        markSeen()
        io {
            val id = activeAccountId
            if (id < 0) return@io
            loadAutoSyncMinutes(id)
            val due = runCatching { MailNative.resumeSyncDue(id) }.getOrDefault(true)
            withContext(Dispatchers.Main) {
                restartAutoSync()
                if (autoSyncMinutes > 0 && !syncing && due) syncAccount(id)
            }
        }
    }

    /**
     * Re-plan the background checks from every account's settings and run
     * the plan (push service, poller, quiet-hours replan). Database only.
     * Called on start and after account changes; boot, app updates and the
     * clock re-plan on their own (MailSchedule).
     */
    internal fun rescheduleBackground() = io {
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
    internal suspend fun loadAutoSyncMinutes(accountId: Long) {
        val minutes = runCatching {
            JSONObject(MailNative.accountSettingsJson(accountId))
                .optJSONObject("effective")
                ?.optString("sync_interval_minutes")
                ?.toIntOrNull()
        }.getOrNull() ?: 0
        withContext(Dispatchers.Main) { autoSyncMinutes = minutes }
    }

    /** Main thread. One timer per state, only while in the foreground. */
    internal fun restartAutoSync() {
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
        if (kind == "Save draft") settleDraft(phase, e.optBoolean("ok", true), e.optString("status"))
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
        if (kind == "Capabilities" && !ok && phase == "finished") {
            val id = e.optLong("account_id", -1).takeIf { it >= 0 } ?: capabilitiesFor
            capabilitiesError = capabilitiesError + (id to text.ifEmpty { "Refresh failed" })
        }
        if (kind == "Capabilities" && ok && phase == "finished") {
            val caps = runCatching { JSONObject(text) }.getOrNull()
            if (caps != null) {
                val id = caps.optLong("account_id", -1)
                capabilities = capabilities + (id to caps)
                capabilitiesError = capabilitiesError - id
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
        refreshSearch()
    }

    /**
     * Cold start only: where a one-pane layout opens, the `start_view`
     * setting (`folders` | `inbox`), once the landing folder is known. The
     * shell applies it once and clears it; a return from the background
     * never sets it.
     */
    var startView by mutableStateOf<String?>(null)
        internal set

    fun consumeStartView() {
        startView = null
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
            // The waiter is in place (main thread), so the queue call itself
            // can go to IO: it is a JNI call whose "queued" event re-enters
            // Java from inside the native frame.
            withContext(Dispatchers.IO) { queue() }
            withTimeoutOrNull(timeoutMs) { waiter.await() }
        } finally {
            finishWaiters[kind]?.remove(cb)
        }
    }

    /** Seconds an offer stays undoable (the core's grace period). */
    val undoGraceSecs: Long by lazy { runCatching { MailNative.undoGraceSecs().toLong() }.getOrDefault(10L) }

    companion object {
        const val PAGE = 200L
        const val TAG = "MailState"
    }
}
