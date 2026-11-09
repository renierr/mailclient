package de.renier.mailclient.ui.state

import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/**
 * Refresh local counts and, while open, the combined inbox (its first
 * [MailState.combinedPages] pages) or its local search. Runs from
 * [loadFolders], so every folder refresh covers it.
 */
fun MailState.refreshCombined() {
    scope.launch(Dispatchers.Main) {
        val generation = ++combinedGeneration
        val query = searchQuery.trim()
        val open = combinedOpen
        val pages = combinedPages
        val result = withContext(Dispatchers.IO) {
            runCatching {
                MailNative.ensureInit(appContext)
                val counts = JSONObject(MailNative.combinedInboxCountsJson())
                val rows = if (!open) emptyList() else if (query.isEmpty()) {
                    parseMessages(MailNative.combinedInboxMessagesJson(pages))
                } else {
                    parseMessages(MailNative.combinedInboxSearchJson(query))
                }
                counts to rows
            }
        }
        if (generation != combinedGeneration) return@launch
        result.onSuccess { (counts, rows) ->
            combinedAccounts = counts.optInt("accounts")
            combinedUnread = counts.optInt("unread")
            combinedTotal = counts.optInt("total")
            if (open && combinedOpen && searchQuery.trim() == query) combinedMessages = rows
        }.onFailure { fail(it.message ?: "Could not load combined inbox") }
    }
}

/** More cached rows exist than the combined list shows (no search running). */
val MailState.combinedHasMore: Boolean
    get() = searchQuery.isEmpty() && combinedMessages.size < combinedTotal

/** One more page of the combined list. */
fun MailState.showMoreCombined() {
    combinedPages++
    refreshCombined()
}

fun MailState.openCombinedInbox() {
    clearSearch()
    exitSelectionMode()
    outboxOpen = false
    combinedPages = 1
    combinedOpen = true
    refreshCombined()
}

fun MailState.closeCombinedInbox() {
    combinedOpen = false
}

/** What the empty combined list says; it only ever searches locally. */
fun MailState.combinedEmptyText(): String = runCatching {
    MailNative.emptyListText(searchQuery.isNotEmpty(), false, false, 0, searchQuery)
}.getOrDefault("")

fun MailState.deleteCombinedMessage(row: MessageRow, confirmedPermanent: Boolean) = io {
    MailNative.ensureInit(appContext)
    val result = try {
        MailNative.combinedDeleteMessage(row.accountId, row.folderId, row.uid, confirmedPermanent)
    } catch (e: Exception) {
        refreshCombined()
        throw e
    }
    withContext(Dispatchers.Main) { offerUndo(result) }
    loadFolders()
}

/**
 * Leave the combined view for the row's own account and inbox — the same
 * switch the account chip makes — then hand over to [onReady].
 */
fun MailState.openCombinedMessage(row: MessageRow, onReady: (Long, Long, Int) -> Unit) = io {
    MailNative.ensureInit(appContext)
    val exists = parseFolders(MailNative.foldersJson(row.accountId)).any { it.id == row.folderId }
    if (!exists) {
        fail("This inbox is no longer available")
        return@io
    }
    if (!withContext(Dispatchers.Main) { combinedOpen }) return@io
    val landed = switchAccount(row.accountId, row.folderId)
    if (landed != row.accountId) {
        fail("This inbox is no longer available")
        return@io
    }
    withContext(Dispatchers.Main) { onReady(landed, row.folderId, row.uid) }
}
