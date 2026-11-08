package de.renier.mailclient.ui.state

import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** Refresh local counts and, while open, the combined inbox or local search. */
fun MailState.refreshCombined() {
    scope.launch(Dispatchers.Main) {
        val generation = ++combinedGeneration
        val query = searchQuery.trim()
        val open = combinedOpen
        val result = withContext(Dispatchers.IO) {
            runCatching {
                MailNative.ensureInit(appContext)
                val counts = JSONObject(MailNative.combinedInboxCountsJson())
                val rows = if (!open) emptyList() else if (query.isEmpty()) {
                    parseMessages(MailNative.combinedInboxMessagesJson(counts.optLong("total"), 0))
                } else {
                    parseMessages(MailNative.combinedInboxSearchJson(query))
                }
                counts to rows
            }
        }
        if (generation != combinedGeneration) return@launch
        result.onSuccess { (counts, rows) ->
            combinedInboxes = counts.optInt("inboxes")
            combinedUnread = counts.optInt("unread")
            if (open && combinedOpen && searchQuery.trim() == query) combinedMessages = rows
        }.onFailure { fail(it.message ?: "Could not load combined inbox") }
    }
}

fun MailState.openCombinedInbox() {
    clearSearch()
    exitSelectionMode()
    outboxOpen = false
    combinedOpen = true
    refreshCombined()
}

fun MailState.closeCombinedInbox() {
    combinedOpen = false
}

fun MailState.deleteCombinedMessage(row: MessageRow, confirmedPermanent: Boolean) = io {
    MailNative.ensureInit(appContext)
    val result = try {
        MailNative.combinedDeleteMessage(row.accountId, row.folderId, row.uid, confirmedPermanent)
    } catch (e: Exception) {
        refreshCombined()
        throw e
    }
    withContext(Dispatchers.Main) { offerUndo(result) }
    refreshCombined()
    loadFolders()
}

/** Land on the real account and folder before opening any reader actions. */
fun MailState.openCombinedMessage(row: MessageRow, onReady: (Long, Long, Int) -> Unit) = io {
    MailNative.ensureInit(appContext)
    val selected = JSONObject(MailNative.selectAccount(row.accountId))
    val account = selected.optLong("account_id", -1)
    val tree = parseFolders(MailNative.foldersJson(account))
    if (account != row.accountId || tree.none { it.id == row.folderId && it.role == "inbox" }) {
        fail("This inbox is no longer available")
        return@io
    }
    val accepted = withContext(Dispatchers.Main) {
        if (!combinedOpen) return@withContext false
        combinedOpen = false
        clearSearch()
        activeAccountId = account
        folderId = row.folderId
        folders = tree
        messages = emptyList()
        shownMessages = emptyList()
        refreshSidebarRows()
        onReady(account, row.folderId, row.uid)
        true
    }
    if (!accepted) return@io
    refreshOutbox()
    reloadMessages()
    loadAutoSyncMinutes(account)
    withContext(Dispatchers.Main) { restartAutoSync() }
}
