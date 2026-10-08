package de.renier.mailclient.ui.state

import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

// Sort, quick filters, multi-select and bulk / single-row actions for
// MailState. Pure move from MailState.kt — call syntax is unchanged.

/** Sort the list; hidden while searching (hits stay newest-first). */
fun MailState.setSort(field: String, descending: Boolean) = io {
    MailNative.ensureInit(appContext)
    MailNative.setSort(field, descending)
    loadSort()
    reloadMessages()
}

internal fun MailState.loadSort() = io {
    MailNative.ensureInit(appContext)
    val o = runCatching { JSONObject(MailNative.settingsJson()) }.getOrDefault(JSONObject())
    withContext(Dispatchers.Main) {
        sortField = o.optString("message_sort_field", "date").ifEmpty { "date" }
        sortDesc = o.optBoolean("message_sort_desc", true)
    }
}

internal val MailState.hasDateFilter: Boolean get() = filterAfter.isNotEmpty() || filterBefore.isNotEmpty()
internal val MailState.hasListFilter: Boolean
    get() = filterUnread || filterStarred || filterAttachments || hasDateFilter

fun MailState.setUnreadOnly(only: Boolean) {
    filterUnread = only
    recomputeShown()
}

fun MailState.setStarredOnly(only: Boolean) {
    filterStarred = only
    recomputeShown()
}

fun MailState.setAttachmentsOnly(only: Boolean) {
    filterAttachments = only
    recomputeShown()
}

/**
 * Apply a typed custom date range after the core has read it
 * (`list_filter::date_range_check`). Null when applied, else why not.
 */
fun MailState.setDateRange(after: String, before: String): String? {
    val o = runCatching { JSONObject(MailNative.dateRangeCheck(after, before)) }
        .getOrElse { return it.message ?: "Not a date range" }
    val error = o.optString("error")
    if (error.isNotEmpty()) return error
    filterAfter = o.optString("after")
    filterBefore = o.optString("before")
    refreshDateLabel()
    recomputeShown()
    return null
}

/** A `today` / `week` / `month` / `older_month` preset from the core. */
fun MailState.applyDatePreset(preset: String) {
    val o = runCatching { JSONObject(MailNative.datePresetRange(preset)) }.getOrDefault(JSONObject())
    filterAfter = o.optString("after")
    filterBefore = o.optString("before")
    refreshDateLabel()
    recomputeShown()
}

fun MailState.clearDateFilter() {
    filterAfter = ""
    filterBefore = ""
    dateFilterLabel = ""
    recomputeShown()
}

fun MailState.clearListFilters() {
    filterUnread = false
    filterStarred = false
    filterAttachments = false
    clearDateFilter()
}

/** The load-older footer's words for the loaded folder (`feed::older_label`). */
internal fun MailState.refreshOlderLabel() {
    val (cached, server) = olderCounts
    olderLabel = if (olderState.isEmpty()) {
        ""
    } else {
        runCatching {
            MailNative.olderLabel(cached.toLong(), server.toLong(), hasListFilter || rowFilterQuery.isNotEmpty())
        }.getOrDefault("")
    }
}

/** What the empty list says (`mailcore::search::empty_list_text`). */
fun MailState.emptyListText(): String = runCatching {
    MailNative.emptyListText(
        searchActive,
        serverSearchPending,
        hasListFilter,
        if (searchActive) searchHits.size else messages.size,
        if (searchActive) searchQuery else rowFilterQuery,
    )
}.getOrDefault("")

/** Rows of the active pane (folder or search), after filters. */
fun MailState.visibleRows(): List<MessageRow> = if (searchActive) shownHits else shownMessages

fun MailState.selectionKey(m: MessageRow): String =
    if (searchActive) "${rowFolderId(m)}:${m.uid}" else m.uid.toString()

fun MailState.enterSelectionMode(withKey: String? = null) {
    selectionMode = true
    selectedKeys = if (withKey != null) setOf(withKey) else emptySet()
}

fun MailState.exitSelectionMode() {
    selectionMode = false
    selectedKeys = emptySet()
}

fun MailState.toggleSelected(key: String) {
    selectedKeys = if (key in selectedKeys) selectedKeys - key else selectedKeys + key
}

fun MailState.selectAllVisible() {
    selectedKeys = visibleRows().map { selectionKey(it) }.toSet()
}

fun MailState.selectUnreadVisible() {
    selectedKeys = visibleRows().filter { it.unread }.map { selectionKey(it) }.toSet()
}

fun MailState.selectStarredVisible() {
    selectedKeys = visibleRows().filter { it.starred }.map { selectionKey(it) }.toSet()
}

fun MailState.invertSelection() {
    val all = visibleRows().map { selectionKey(it) }.toSet()
    selectedKeys = all - selectedKeys
}

/** Drop keys that are no longer on screen (folder change, sync, filter). */
internal fun MailState.pruneSelection() {
    if (selectedKeys.isEmpty()) return
    val live = visibleRows().map { selectionKey(it) }.toSet()
    selectedKeys = selectedKeys.intersect(live)
    if (selectedKeys.isEmpty()) selectionMode = false
}

val MailState.selectionCount: Int get() = selectedKeys.size

/** The selected rows, resolved against the active pane. */
private fun MailState.selectedRows(): List<MessageRow> {
    val keys = selectedKeys
    if (keys.isEmpty()) return emptyList()
    return visibleRows().filter { selectionKey(it) in keys }
}

val MailState.selectionAllStarred: Boolean get() = selectedRows().all { it.starred }

/** Bulk delete of the selection: destroys when any row's folder would. */
val MailState.selectionDeletePrompt: DeletePrompt
    get() = deletePrompt(bulk = true, selectedRows().map { rowFolder(it) })

/**
 * The core's delete-confirm rule over the target folders (`null`: not
 * in the feed, which the core treats as destroying).
 */
fun MailState.deletePrompt(bulk: Boolean, folders: List<Folder?>): DeletePrompt {
    val flags = folders.joinToString(",", "[", "]") { f -> f?.deleteIsPermanent?.toString() ?: "null" }
    val o = runCatching { JSONObject(MailNative.deletePrompt(readerPrefs.confirmDelete, bulk, flags)) }.getOrNull()
    return DeletePrompt(o?.optBoolean("permanent", true) ?: true, o?.optBoolean("ask", true) ?: true)
}

/** The folder a row lives in: hits carry their own, rows use the open one. */
private fun MailState.rowFolder(m: MessageRow): Folder? {
    val id = rowFolderId(m)
    return folders.firstOrNull { it.id == id }
}

private fun MailState.selectionUidsJson(): String =
    "[${selectedRows().joinToString(",") { it.uid.toString() }}]"

/** `[{"folder": path, "uid": n}]` for the `*Hits` bulk calls. */
private fun MailState.selectionHitsJson(): String {
    val arr = org.json.JSONArray()
    for (m in selectedRows()) {
        val path = rowFolder(m)?.path ?: continue
        arr.put(JSONObject().put("folder", path).put("uid", m.uid))
    }
    return arr.toString()
}

fun MailState.bulkMarkRead(read: Boolean) = io {
    MailNative.ensureInit(appContext)
    if (searchActive) {
        MailNative.markReadHits(activeAccountId, selectionHitsJson(), read)
    } else {
        MailNative.markReadMany(activeAccountId, folderId, selectionUidsJson(), read)
    }
    afterBulk(keepSelection = true)
}

fun MailState.bulkStar(starred: Boolean) = io {
    MailNative.ensureInit(appContext)
    if (searchActive) {
        MailNative.setStarHits(activeAccountId, selectionHitsJson(), starred)
    } else {
        MailNative.setStarMany(activeAccountId, folderId, selectionUidsJson(), starred)
    }
    afterBulk(keepSelection = true)
}

fun MailState.bulkArchive() = io {
    MailNative.ensureInit(appContext)
    val result = if (searchActive) {
        MailNative.archiveHits(activeAccountId, selectionHitsJson())
    } else {
        MailNative.archiveMessages(activeAccountId, folderId, selectionUidsJson())
    }
    withContext(Dispatchers.Main) { offerUndo(result) }
    afterBulk()
}

fun MailState.bulkMove(destPath: String) = io {
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
fun MailState.bulkTrash() = io {
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
fun MailState.bulkPurge() = io {
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

/** The id of the folder [m] lives in. */
fun MailState.rowFolderId(m: MessageRow): Long = if (searchActive && m.folderId >= 0) m.folderId else folderId

/** Deleting [m] alone: destroys or moves to Trash, and whether to ask. */
fun MailState.rowDeletePrompt(m: MessageRow): DeletePrompt = deletePrompt(bulk = false, listOf(rowFolder(m)))

/** Delete in [m]'s folder destroys instead of moving to Trash. */
fun MailState.rowDeleteIsPermanent(m: MessageRow): Boolean = rowDeletePrompt(m).permanent

fun MailState.rowMarkRead(m: MessageRow, read: Boolean) = io {
    MailNative.ensureInit(appContext)
    MailNative.markReadMany(activeAccountId, rowFolderId(m), "[${m.uid}]", read)
    afterRow()
}

fun MailState.rowStar(m: MessageRow, starred: Boolean) = io {
    MailNative.ensureInit(appContext)
    MailNative.setStarMany(activeAccountId, rowFolderId(m), "[${m.uid}]", starred)
    afterRow()
}

fun MailState.rowArchive(m: MessageRow) = io {
    MailNative.ensureInit(appContext)
    val result = MailNative.archiveMessages(activeAccountId, rowFolderId(m), "[${m.uid}]")
    withContext(Dispatchers.Main) { offerUndo(result) }
    afterRow()
}

fun MailState.rowMove(m: MessageRow, destPath: String) = io {
    MailNative.ensureInit(appContext)
    val result = MailNative.moveMessages(activeAccountId, rowFolderId(m), "[${m.uid}]", destPath)
    withContext(Dispatchers.Main) { offerUndo(result) }
    afterRow()
}

/** To Trash (undoable), or destroyed where the folder says so. */
fun MailState.rowTrash(m: MessageRow) = io {
    MailNative.ensureInit(appContext)
    val result = MailNative.deleteMessages(activeAccountId, rowFolderId(m), "[${m.uid}]")
    withContext(Dispatchers.Main) { offerUndo(result) }
    afterRow()
}

/** Destroy server-side. No undo — the UI always confirms first. */
fun MailState.rowPurge(m: MessageRow) = io {
    MailNative.ensureInit(appContext)
    MailNative.purgeMessages(activeAccountId, rowFolderId(m), "[${m.uid}]")
    afterRow()
}

private suspend fun MailState.afterRow() {
    reloadMessages()
    loadFolders()
    withContext(Dispatchers.Main) { refreshSearch() }
}

// Mark read / star keep the selection so actions chain (Qt); actions that
// take the rows away end selection mode.
private suspend fun MailState.afterBulk(keepSelection: Boolean = false) {
    if (!keepSelection) withContext(Dispatchers.Main) { exitSelectionMode() }
    reloadMessages()
    loadFolders()
    refreshSearch()
}
