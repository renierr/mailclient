package de.renier.mailclient.ui.state

import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

// Search, find-similar and the client-side row/filtered views for MailState.
// Pure move from MailState.kt — call syntax is unchanged.

fun MailState.setSearch(query: String) {
    exitSelectionMode()
    serverSearchFired = false
    serverSearchPending = false
    searchQuery = query
    runSearch()
}

fun MailState.toggleSearchScope() {
    searchFolderOnly = !searchFolderOnly
    // Fresh scope, fresh server top-up (Qt `folderScopeToggled`).
    serverSearchFired = false
    serverSearchPending = false
    if (similarTarget == null) runSearch()
}

fun MailState.clearSearch() {
    searchJob?.cancel()
    exitSelectionMode()
    serverSearchFired = false
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
        if (rowFilterQuery.isNotEmpty()) {
            rowFilterQuery = ""
            recomputeShown()
        }
        searchHits = hits
        searchActive = true
        shownHits = hits.filter { rowShown(it) }
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

/** AND-combined quick filters over one row; never fetches. */
private fun MailState.rowShown(m: MessageRow): Boolean {
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

/** The typed row filter over one folder row (core match rule). */
private fun MailState.rowFilterPasses(m: MessageRow): Boolean =
    rowFilterQuery.isEmpty() ||
        MailNative.searchFilterMatches(rowFilterQuery, m.subject, m.from, m.fromName, m.snippet) == "true"

/** What the list paints; the backing rows stay untouched. */
internal fun MailState.recomputeShown() {
    shownMessages = messages.filter { rowShown(it) && rowFilterPasses(it) }
    shownHits = searchHits.filter { rowShown(it) }
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
            rowFilterQuery = filter
            recomputeShown()
        }
    }
}
