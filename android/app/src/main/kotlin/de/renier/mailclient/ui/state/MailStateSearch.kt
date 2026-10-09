package de.renier.mailclient.ui.state

import android.util.Log
import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

// Search, find-similar and the client-side row/filtered views for MailState.

fun MailState.setSearch(query: String) {
    exitSelectionMode()
    serverSearchFired.set(false)
    serverSearchPending = false
    searchQuery = query
    runSearch()
}

fun MailState.toggleSearchScope() {
    searchFolderOnly = !searchFolderOnly
    // Fresh scope, fresh server top-up (Qt `folderScopeToggled`).
    serverSearchFired.set(false)
    serverSearchPending = false
    if (similarTarget == null) runSearch()
}

fun MailState.clearSearch() {
    searchJob?.cancel()
    exitSelectionMode()
    serverSearchFired.set(false)
    serverSearchPending = false
    searchQuery = ""
    searchHits = emptyList()
    searchActive = false
    similarLabel = null
    similarTarget = null
    if (rowFilterQuery.isNotEmpty()) {
        rowFilterQuery = ""
        recomputeShown()
    }
}

/** Mail like this one across the account, shown as list hits. */
fun MailState.findSimilar(folder: Long, uid: Int) = loadSimilar(folder, uid, refresh = false)

/**
 * Re-read whatever the hits show — the query, or the find-similar
 * result — after a row, reader, undo or job change. Main thread.
 */
internal fun MailState.refreshSearch() {
    if (!searchActive) return
    val t = similarTarget
    if (t != null) loadSimilar(t.first, t.second, refresh = true) else runSearch()
}

internal fun MailState.loadSimilar(folder: Long, uid: Int, refresh: Boolean) = io {
    val accountId = activeAccountId
    // A refresh whose target is gone keeps the hits it has.
    val hits = runCatching { parseMessages(MailNative.similarJson(accountId, folder, uid)) }
        .getOrElse { if (refresh) return@io else throw it }
    val subject = runCatching { MailNative.similarSubject(accountId, folder, uid) }.getOrDefault("")
    withContext(Dispatchers.Main) {
        if (refresh && similarTarget != folder to uid) return@withContext
        searchJob?.cancel()
        if (refresh) pruneSelection() else exitSelectionMode()
        searchQuery = ""
        rowFilterQuery = ""
        searchHits = hits
        searchActive = true
        recomputeShown()
        similarTarget = folder to uid
        if (!refresh || similarLabel == null) {
            similarLabel = "Similar to: ${subject.ifEmpty { "this message" }}"
        }
    }
}

internal fun MailState.refreshDateLabel() {
    dateFilterLabel =
        if (hasDateFilter) MailNative.dateFilterLabel(filterAfter, filterBefore) else ""
}

/** The active list filters with the typed row filter [text], as the core reads them. */
private fun MailState.listFilterJson(text: String): String = JSONObject()
    .put("unread", filterUnread)
    .put("starred", filterStarred)
    .put("attachments", filterAttachments)
    .put("after", filterAfter)
    .put("before", filterBefore)
    .put("text", text)
    .toString()

/**
 * The folder [rows] the list filters keep, decided by the core over the
 * folder's cached rows (`list_filter::keep_in_folder`): only the filter
 * goes over, the kept uids come back. IO thread.
 */
private fun keptMessages(folder: Long, rows: List<MessageRow>, filter: String): List<MessageRow> {
    val kept = runCatching { JSONArray(MailNative.listFilterUids(folder, filter)) }
        .getOrElse {
            Log.w(MailState.TAG, "list filter failed", it)
            return rows
        }
    val keep = HashSet<Int>(kept.length())
    for (i in 0 until kept.length()) keep.add(kept.getInt(i))
    return rows.filter { it.uid in keep }
}

/**
 * The search [hits] the quick filters keep. Hits are few and already
 * matched by their query, so they go over as rows with no text
 * (`list_filter::keep_json`). IO thread.
 */
private fun keptHits(hits: List<MessageRow>, filter: String): List<MessageRow> {
    val arr = JSONArray()
    for (m in hits) {
        arr.put(
            JSONObject()
                .put("date_raw", m.dateRaw)
                .put("unread", m.unread)
                .put("starred", m.starred)
                .put("has_attachments", m.hasAttachments),
        )
    }
    val kept = runCatching { JSONArray(MailNative.listFilterKeep(filter, arr.toString())) }
        .getOrElse {
            Log.w(MailState.TAG, "list filter failed", it)
            return hits
        }
    return List(kept.length()) { hits[kept.getInt(it)] }
}

/**
 * What the list paints; the backing rows stay untouched. Main thread.
 * Unfiltered it is immediate; a filter reads the cache, so it runs on IO
 * and a newer call supersedes one still running.
 */
internal fun MailState.recomputeShown() {
    filterJob?.cancel()
    val rows = messages
    val hits = searchHits
    val text = rowFilterQuery
    if ((!hasListFilter && text.isEmpty()) || (rows.isEmpty() && hits.isEmpty())) {
        applyShown(rows, hits)
        return
    }
    val folder = folderId
    val filter = listFilterJson(text)
    val hitFilter = if (hasListFilter) listFilterJson("") else null
    filterJob = scope.launch(Dispatchers.IO) {
        MailNative.ensureInit(appContext)
        val shown = if (rows.isEmpty()) rows else keptMessages(folder, rows, filter)
        val shownHitRows = if (hitFilter == null || hits.isEmpty()) hits else keptHits(hits, hitFilter)
        withContext(Dispatchers.Main) {
            // Stale if the folder or the rows changed meanwhile.
            if (folderId != folder || messages !== rows || searchHits !== hits) return@withContext
            applyShown(shown, shownHitRows)
        }
    }
}

private fun MailState.applyShown(rows: List<MessageRow>, hits: List<MessageRow>) {
    shownMessages = rows
    shownHits = hits
    refreshOlderLabel()
    pruneSelection()
}

internal fun MailState.runSearch() {
    searchJob?.cancel()
    similarLabel = null
    similarTarget = null
    val query = searchQuery
    val accountId = activeAccountId
    val folderScope = if (searchFolderOnly) openFolder?.path.orEmpty() else ""
    searchJob = scope.launch(Dispatchers.IO) {
        MailNative.ensureInit(appContext)
        val plan = runCatching { JSONObject(MailNative.searchPlan(query)) }.getOrNull()
        val filter = if (plan?.optString("mode") == "filter") plan.optString("query") else ""
        val rows = runCatching { when (plan?.optString("mode")) {
            // The local index answers every keystroke; only the server
            // backfill waits out the core's debounce.
            "indexed" -> {
                val found = parseMessages(MailNative.searchJson(accountId, plan.optString("query"), folderScope))
                // Thin index results trigger one queued server backfill
                // per query; its finish event re-runs this search.
                if (found.size < plan.optInt("hit_limit", 50) && serverSearchFired.compareAndSet(false, true)) {
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
            rowFilterQuery = filter
            recomputeShown()
        }
    }
}
