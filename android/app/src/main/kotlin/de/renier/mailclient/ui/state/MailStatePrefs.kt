package de.renier.mailclient.ui.state

import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

// Reader/poll prefs, capabilities, outbox, undo and account removal for
// MailState. Pure move from MailState.kt — call syntax is unchanged.

fun MailState.loadReaderPrefs() = io {
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
        scale = runCatching { MailNative.readerTextScale(o.optString("reader_font_size")) }.getOrDefault(1f),
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
fun MailState.settingsSaved() {
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
fun MailState.refreshCapabilities(accountId: Long) = io {
    MailNative.ensureInit(appContext)
    capabilitiesFor = accountId
    withContext(Dispatchers.Main) { capabilitiesError = capabilitiesError - accountId }
    try {
        MailNative.refreshServerCapabilities(accountId)
    } catch (e: Exception) {
        if (!e.isAlreadyRunning()) {
            val msg = e.message ?: "Could not ask the server"
            withContext(Dispatchers.Main) { capabilitiesError = capabilitiesError + (accountId to msg) }
            fail(msg)
        }
    }
}

/** Take back the current offer (the snackbar's Undo, Ctrl+Z). Main thread. */
fun MailState.undo() {
    val batch = undoOffer?.batch
    undoOffer = null
    if (!batch.isNullOrEmpty()) undoBatch(batch)
}

internal fun MailState.undoBatch(batch: String) = io {
    MailNative.ensureInit(appContext)
    val text = runCatching { MailNative.undoMove(batch) }
        .getOrElse { it.message ?: "undo failed" }
    putStatus(text, false)
    reloadMessages()
    // The rows are visible again: the tree counts must come back too,
    // like the Flutter undo does (messages + folders + hits).
    loadFolders()
    refreshSearch()
}

internal fun MailState.offerUndo(resultJson: String) {
    val o = runCatching { JSONObject(resultJson) }.getOrNull() ?: return
    val batch = o.optString("batch")
    if (batch.isEmpty()) return
    undoOffer = UndoOffer(batch, o.optString("label"))
}

/** Delete with folders, messages and secrets; land on the next account. */
fun MailState.removeAccount(id: Long) = io {
    MailNative.ensureInit(appContext)
    runCatching { MailNative.deleteAccount(id) }
        .onFailure { fail(it.message ?: "delete failed") }
    refreshAll()
}

fun MailState.markSeen() = io {
    runCatching {
        MailNative.ensureInit(appContext)
        MailNative.backgroundMarkSeen()
    }
}

fun MailState.refreshOutbox() = io {
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
