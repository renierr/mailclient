package de.renier.mailclient.ui.list

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.ui.common.MessageDeleteConfirm
import de.renier.mailclient.ui.common.PullToSync
import de.renier.mailclient.ui.common.rememberEmlSaver
import de.renier.mailclient.ui.folders.MoveToDialog
import de.renier.mailclient.ui.state.Folder
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.MessageRow
import de.renier.mailclient.ui.state.findSimilar
import de.renier.mailclient.ui.state.deleteCombinedMessage
import de.renier.mailclient.ui.state.openCombinedMessage
import de.renier.mailclient.ui.state.parseFolders
import de.renier.mailclient.ui.state.rowArchive
import de.renier.mailclient.ui.state.rowDeleteIsPermanent
import de.renier.mailclient.ui.state.rowDeletePrompt
import de.renier.mailclient.ui.state.rowMarkRead
import de.renier.mailclient.ui.state.rowMove
import de.renier.mailclient.ui.state.rowPurge
import de.renier.mailclient.ui.state.rowStar
import de.renier.mailclient.ui.state.syncNow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** The normal message row, with the originating account beside each message. */
@Composable
fun CombinedInbox(state: MailState, onOpen: (MessageRow) -> Unit) {
    var moving by remember { mutableStateOf<MessageRow?>(null) }
    var deleting by remember { mutableStateOf<MessageRow?>(null) }
    var purging by remember { mutableStateOf<MessageRow?>(null) }
    var destinations by remember { mutableStateOf<List<Folder>>(emptyList()) }
    val saver = rememberEmlSaver(state, onSaved = { state.info("Saved") }, onFailed = { state.info(it) })

    LaunchedEffect(moving?.accountId) {
        val account = moving?.accountId ?: return@LaunchedEffect
        destinations = emptyList()
        destinations = withContext(Dispatchers.IO) {
            runCatching { parseFolders(MailNative.foldersJson(account)).filter { it.subscribed } }
                .getOrDefault(emptyList())
        }
    }
    moving?.let { row ->
        MoveToDialog(destinations, row.folderId, 1, row.subject, onPick = {
            moving = null
            state.rowMove(row, it.path)
        }, onDismiss = { moving = null })
    }
    deleting?.let { row ->
        MessageDeleteConfirm(row.subject, 1, state.rowDeleteIsPermanent(row), onConfirm = {
            deleting = null
            state.deleteCombinedMessage(row, state.rowDeleteIsPermanent(row))
        }, onDismiss = { deleting = null })
    }
    purging?.let { row ->
        MessageDeleteConfirm(row.subject, 1, true, onConfirm = {
            purging = null
            state.rowPurge(row)
        }, onDismiss = { purging = null })
    }

    fun trash(row: MessageRow) {
        if (state.rowDeletePrompt(row).ask) deleting = row else state.deleteCombinedMessage(row, false)
    }
    fun action(action: RowAction, row: MessageRow) {
        when (action) {
            RowAction.Read -> state.rowMarkRead(row, row.unread)
            RowAction.Star -> state.rowStar(row, !row.starred)
            RowAction.Archive -> state.rowArchive(row)
            RowAction.Move -> moving = row
            RowAction.Trash -> trash(row)
            RowAction.Purge -> purging = row
            RowAction.Similar -> state.openCombinedMessage(row) { _, folder, uid -> state.findSimilar(folder, uid) }
            RowAction.SaveEml -> saver(row.folderId, row.uid)
        }
    }
    fun swipe(action: String, row: MessageRow) {
        when (action) {
            "archive" -> state.rowArchive(row)
            "delete" -> trash(row)
            "mark_read" -> state.rowMarkRead(row, true)
            "mark_unread" -> state.rowMarkRead(row, false)
            "star" -> state.rowStar(row, true)
            "unstar" -> state.rowStar(row, false)
        }
    }

    Column(Modifier.fillMaxSize()) {
        ListHeaderRow(
            state = state,
            title = if (state.searchQuery.isEmpty()) "Combined Inbox" else "Search results",
            subtitle = "${state.combinedMessages.size} messages",
            onCustomRange = {},
            combined = true,
        )
        if (state.combinedMessages.isEmpty()) {
            Text("No cached messages", modifier = Modifier.padding(16.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        PullToSync(onSync = { state.syncNow() }, modifier = Modifier.weight(1f)) {
            LazyColumn(Modifier.fillMaxSize()) {
                items(state.combinedMessages, key = { "${it.folderId}:${it.uid}" }) { row ->
                    val account = row.accountEmail.ifEmpty { state.accounts.firstOrNull { it.id == row.accountId }?.email.orEmpty() }
                    SwipeableRow(state.swipeLeftAction, state.swipeRightAction, enabled = true, onAction = { swipe(it, row) }) {
                        MessageItem(
                            m = row,
                            folderLabel = account,
                            selected = null,
                            modifier = Modifier.clickable { onOpen(row) },
                            onToggle = {},
                            compact = state.compactList,
                            permanent = state.rowDeleteIsPermanent(row),
                            onAction = { action(it, row) },
                        )
                    }
                }
            }
        }
    }
}
