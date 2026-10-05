package de.renier.mailclient.ui.state

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import de.renier.mailclient.JobEvents
import de.renier.mailclient.MailNative
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
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
    // Core-decided avatar (mailcore::badge): initials + per-theme hex.
    val initials: String = "?",
    val avatarLight: String = "",
    val avatarDark: String = "",
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

    val activeAccount: Account? get() = accounts.firstOrNull { it.id == activeAccountId }
    val openFolder: Folder? get() = folders.firstOrNull { it.id == folderId }

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
        syncing = false
        val accountId = e.optLong("account_id", -1)
        val eventFolder = e.optLong("folder_id", -1)
        // Re-read whatever is showing, like the Dart side does.
        if (kind == "Folders" || (accountId == activeAccountId && eventFolder == -1L)) {
            refreshFolders(andMessages = true)
        } else if (accountId == activeAccountId && (eventFolder == folderId || eventFolder == -1L)) {
            reloadMessages()
        }
        refreshOutbox()
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
        if (folderId >= 0) reloadMessages()
        putStatus(if (accountId < 0) "No accounts yet" else "Ready", false)
    }

    fun selectAccount(id: Long) = io {
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
        folderId = id
        messages = emptyList()
        canLoadOlder = false
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
        val rows = parseMessages(MailNative.messagesJson(id, PAGE, 0))
        val counts = JSONObject(MailNative.folderCounts(id))
        withContext(Dispatchers.Main) {
            if (folderId == id) {
                messages = rows
                canLoadOlder = counts.optBoolean("can_load_older", false)
            }
        }
    }

    fun loadMore() = io {
        MailNative.ensureInit(appContext)
        runCatching { MailNative.loadOlderMessages(activeAccountId, folderId) }
            .onFailure { fail(it.message ?: "load older failed") }
        // The finished event reloads the page; rows appear then.
    }

    fun syncNow() = io {
        MailNative.ensureInit(appContext)
        val id = activeAccountId
        if (id < 0) {
            putStatus("No accounts yet", true)
            return@io
        }
        withContext(Dispatchers.Main) { syncing = true }
        runCatching { MailNative.syncAccount(id) }
            .onFailure { fail(it.message ?: "sync failed") }
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
                    leaf = o.optString("leaf").ifEmpty { path.substringAfterLast(o.optString("delimiter", "/")) },
                    depth = o.optInt("depth", 0),
                    role = o.optString("role"),
                    unread = o.optInt("unread", 0),
                    count = o.optInt("count", 0),
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
                    initials = o.optString("initials", "?"),
                    avatarLight = o.optString("avatar_light"),
                    avatarDark = o.optString("avatar_dark"),
                )
            }.filter { it.uid >= 0 }
        }
    }
}
