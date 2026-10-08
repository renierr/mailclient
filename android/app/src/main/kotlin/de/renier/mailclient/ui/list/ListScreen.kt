package de.renier.mailclient.ui.list

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.ui.common.MessageDeleteConfirm
import de.renier.mailclient.ui.common.PullToSync
import de.renier.mailclient.ui.common.rememberEmlSaver
import de.renier.mailclient.ui.folders.MoveToDialog
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.MessageRow
import de.renier.mailclient.ui.state.bulkMove
import de.renier.mailclient.ui.state.bulkPurge
import de.renier.mailclient.ui.state.bulkTrash
import de.renier.mailclient.ui.state.clearSearch
import de.renier.mailclient.ui.state.deletePrompt
import de.renier.mailclient.ui.state.emptyListText
import de.renier.mailclient.ui.state.enterSelectionMode
import de.renier.mailclient.ui.state.findSimilar
import de.renier.mailclient.ui.state.hasListFilter
import de.renier.mailclient.ui.state.loadMore
import de.renier.mailclient.ui.state.openFolder
import de.renier.mailclient.ui.state.rowArchive
import de.renier.mailclient.ui.state.rowDeleteIsPermanent
import de.renier.mailclient.ui.state.rowDeletePrompt
import de.renier.mailclient.ui.state.rowFolderId
import de.renier.mailclient.ui.state.rowMarkRead
import de.renier.mailclient.ui.state.rowMove
import de.renier.mailclient.ui.state.rowPurge
import de.renier.mailclient.ui.state.rowStar
import de.renier.mailclient.ui.state.rowTrash
import de.renier.mailclient.ui.state.selectionCount
import de.renier.mailclient.ui.state.selectionDeletePrompt
import de.renier.mailclient.ui.state.selectionKey
import de.renier.mailclient.ui.state.syncFolder
import de.renier.mailclient.ui.state.syncNow
import de.renier.mailclient.ui.state.toggleSelected
import de.renier.mailclient.ui.state.visibleFolders

// Message list: a pinned header (folder or search title, sort, filter,
// selection), an active-filter bar, the rows (MessageItem), and the "load
// older" tail. Jump buttons float over long lists; the bulk bar docks at
// the bottom while selected. Header pieces live in ListHeader, the footer,
// empty state and jump buttons in ListOverlays.
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ListScreen(
    state: MailState,
    onOpenReader: (Long, Long, Int) -> Unit,
    // (folderId, uid) of the message open beside the list, highlighted.
    openMessage: Pair<Long, Int>? = null,
) {
    val folder = state.openFolder
    val searching = state.searchActive
    if (folder == null && !searching) {
        Text(
            "No folder open.",
            modifier = Modifier.padding(16.dp),
            style = MaterialTheme.typography.bodyMedium,
        )
        return
    }
    val rows = if (searching) state.shownHits else state.shownMessages
    val total = if (searching) state.searchHits.size else state.messages.size
    // Account-wide hits name their folder; in-folder rows need not.
    val showFolder = searching && !state.searchFolderOnly
    val folderNames = remember(state.folders) { state.folders.associate { it.id to it.leaf } }
    // Search/similar hits arrive grouped by folder (core keeps each
    // folder's hits together in first-hit order): render a section header
    // per group like Qt and Flutter, instead of a flat list.
    val entries: List<ListEntry> = remember(rows, searching, showFolder, folderNames) {
        if (!searching || !showFolder) {
            rows.map { ListEntry.Row(it) }
        } else {
            buildList {
                var lastFolder = Long.MIN_VALUE
                for (m in rows) {
                    val fid = state.rowFolderId(m)
                    if (fid != lastFolder) {
                        lastFolder = fid
                        add(ListEntry.Header(fid, folderNames[fid] ?: "?"))
                    }
                    add(ListEntry.Row(m))
                }
            }
        }
    }
    // Whether a row's delete destroys depends on its folder alone: asked
    // of the core once per folder, not per row on every recomposition.
    val permanentIn = remember(state.folders) {
        state.folders.associate { it.id to state.deletePrompt(bulk = false, listOf(it)).permanent }
    }
    // Scroll memory: one position per folder (search has its own), restored
    // when the list is rebuilt after the reader or a folder switch, saved
    // when it leaves the composition.
    val scrollKey = if (searching) "search" else "folder:${folder?.id ?: -1}"
    val saved = remember(scrollKey) { state.listScrollFor(scrollKey) }
    val listState = remember(scrollKey) {
        LazyListState(
            firstVisibleItemIndex = saved?.first ?: 0,
            firstVisibleItemScrollOffset = saved?.second ?: 0,
        )
    }
    DisposableEffect(scrollKey) {
        onDispose {
            state.saveListScroll(
                scrollKey,
                listState.firstVisibleItemIndex,
                listState.firstVisibleItemScrollOffset,
            )
        }
    }

    var dateDialog by remember { mutableStateOf(false) }
    var bulkMove by remember { mutableStateOf(false) }
    var confirmTrash by remember { mutableStateOf(false) }
    var confirmPurge by remember { mutableStateOf(false) }
    // The ⋮ menu's pending row action: move picker or delete confirm.
    var rowMove by remember { mutableStateOf<MessageRow?>(null) }
    var rowTrash by remember { mutableStateOf<MessageRow?>(null) }
    var rowPurge by remember { mutableStateOf<MessageRow?>(null) }
    val emlSaver = rememberEmlSaver(
        onSaved = { state.info("Saved") },
        onFailed = { state.info(it) },
    )

    fun onRowAction(action: RowAction, m: MessageRow) {
        when (action) {
            RowAction.Read -> state.rowMarkRead(m, m.unread)
            RowAction.Star -> state.rowStar(m, !m.starred)
            RowAction.Archive -> state.rowArchive(m)
            RowAction.Move -> rowMove = m
            // Whether to ask is the core's rule (preference, or no undo).
            RowAction.Trash -> if (state.rowDeletePrompt(m).ask) rowTrash = m else state.rowTrash(m)
            RowAction.Purge -> rowPurge = m
            RowAction.Similar -> state.findSimilar(state.rowFolderId(m), m.uid)
            RowAction.SaveEml -> emlSaver(state.rowFolderId(m), m.uid)
        }
    }

    rowMove?.let { m ->
        MoveToDialog(
            folders = state.visibleFolders,
            currentFolderId = state.rowFolderId(m),
            count = 1,
            subject = m.subject,
            onPick = { dest ->
                rowMove = null
                state.rowMove(m, dest.path)
            },
            onDismiss = { rowMove = null },
        )
    }
    rowTrash?.let { m ->
        MessageDeleteConfirm(
            subject = m.subject,
            count = 1,
            permanent = state.rowDeleteIsPermanent(m),
            onConfirm = {
                rowTrash = null
                state.rowTrash(m)
            },
            onDismiss = { rowTrash = null },
        )
    }
    rowPurge?.let { m ->
        MessageDeleteConfirm(
            subject = m.subject,
            count = 1,
            permanent = true,
            onConfirm = {
                rowPurge = null
                state.rowPurge(m)
            },
            onDismiss = { rowPurge = null },
        )
    }

    if (bulkMove) {
        // One shared folder when the selection sits in it, else none
        // disabled — search selections can span folders.
        val singleFolder = if (searching) {
            rows.filter { state.selectedKeys.contains(state.selectionKey(it)) }
                .map { state.rowFolderId(it) }
                .toSet().singleOrNull() ?: -1
        } else {
            state.folderId
        }
        MoveToDialog(
            folders = state.visibleFolders,
            currentFolderId = singleFolder,
            count = state.selectionCount,
            subject = null,
            onPick = { dest ->
                bulkMove = false
                state.bulkMove(dest.path)
            },
            onDismiss = { bulkMove = false },
        )
    }
    if (confirmTrash || confirmPurge) {
        // The core always asks for a bulk delete; it destroys when any
        // selected row's folder would.
        MessageDeleteConfirm(
            subject = null,
            count = state.selectionCount,
            permanent = confirmPurge || state.selectionDeletePrompt.permanent,
            onConfirm = {
                if (confirmPurge) state.bulkPurge() else state.bulkTrash()
                confirmTrash = false
                confirmPurge = false
            },
            onDismiss = {
                confirmTrash = false
                confirmPurge = false
            },
        )
    }
    if (dateDialog) {
        DateRangeDialog(state) { dateDialog = false }
    }

    PullToSync(
        // Inside a folder only that folder refreshes; account-wide search
        // pulls the whole account like the Sync entry.
        onSync = { if (state.searchActive) state.syncNow() else state.syncFolder() },
    ) {
        Column(modifier = Modifier.fillMaxSize()) {
            ListHeaderRow(
                state = state,
                title = if (searching) {
                    // The bar below names the message; the title stays short.
                    if (state.similarLabel != null) "Similar messages" else "Search results"
                } else {
                    folder?.leaf.orEmpty()
                },
                subtitle = when {
                    searching && total == 0 && state.serverSearchPending -> "Searching server…"
                    searching && total == 0 -> "No matches"
                    searching -> "${rows.size} found" +
                        if (state.searchFolderOnly && folder != null) " in ${folder.leaf}" else ""
                    folder != null && (state.hasListFilter || state.rowFilterQuery.isNotEmpty()) ->
                        "${rows.size} of $total shown"
                    folder != null && folder.unread > 0 -> "${folder.count} messages · ${folder.unread} unread"
                    folder != null -> "${folder.count} messages"
                    else -> ""
                },
                onCustomRange = { dateDialog = true },
            )
            state.similarLabel?.let { label -> SimilarBar(label) { state.clearSearch() } }
            if (state.hasListFilter) {
                FilterBar(state)
            }
            Box(modifier = Modifier.weight(1f)) {
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize(),
                    // A little room so the last row scrolls clear of the
                    // floating jump buttons (Flutter keeps none at all).
                    contentPadding = PaddingValues(bottom = 16.dp),
                ) {
                    items(
                        entries,
                        key = { if (it is ListEntry.Header) "h:${it.folderId}" else state.selectionKey((it as ListEntry.Row).row) },
                    ) { entry ->
                        if (entry is ListEntry.Header) {
                            Text(
                                entry.label,
                                style = MaterialTheme.typography.labelLarge,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 4.dp),
                            )
                            return@items
                        }
                        val m = (entry as ListEntry.Row).row
                        val key = state.selectionKey(m)
                        val rowFolder = state.rowFolderId(m)
                        MessageItem(
                            m = m,
                            folderLabel = if (showFolder) folderNames[rowFolder] else null,
                            selected = if (state.selectionMode) key in state.selectedKeys else null,
                            compact = state.compactList,
                            permanent = permanentIn[rowFolder] ?: state.rowDeleteIsPermanent(m),
                            onAction = { onRowAction(it, m) },
                            modifier = Modifier
                                .then(
                                    if (openMessage == rowFolder to m.uid) {
                                        Modifier.background(MaterialTheme.colorScheme.secondaryContainer)
                                    } else {
                                        Modifier
                                    },
                                )
                                .combinedClickable(
                                    onClick = {
                                        if (state.selectionMode) {
                                            state.toggleSelected(key)
                                        } else {
                                            onOpenReader(state.activeAccountId, rowFolder, m.uid)
                                        }
                                    },
                                    onLongClick = {
                                        if (!state.selectionMode) state.enterSelectionMode(key)
                                    },
                                    onLongClickLabel = "Select message",
                                ),
                            onToggle = { state.toggleSelected(key) },
                        )
                    }
                    if (!searching && state.olderState.isNotEmpty() && state.olderState != "empty") {
                        item {
                            LoadOlderFooter(
                                label = state.olderLabel,
                                unchecked = state.olderState == "unchecked",
                                canLoad = state.canLoadOlder,
                                busy = state.busy,
                                onLoad = { state.loadMore() },
                            )
                        }
                    }
                }
                if (rows.isEmpty()) {
                    EmptyList(
                        text = state.emptyListText(),
                        // Nothing filtered away: the folder itself is empty.
                        syncHint = !searching && !state.hasListFilter && state.rowFilterQuery.isEmpty(),
                        modifier = Modifier.align(Alignment.Center),
                    )
                }
                ScrollJumpButtons(
                    listState,
                    modifier = Modifier.align(Alignment.BottomEnd).padding(end = 16.dp, bottom = 16.dp),
                )
            }
            if (state.selectionMode && state.selectionCount > 0) {
                ListBulkBar(
                    state = state,
                    onMove = { bulkMove = true },
                    onTrash = { if (state.selectionDeletePrompt.ask) confirmTrash = true else state.bulkTrash() },
                    onPurge = { confirmPurge = true },
                )
            }
        }
    }
}

// One visible list entry: a folder section header over account-wide
// search/similar hits, or a message row.
sealed interface ListEntry {
    data class Header(val folderId: Long, val label: String) : ListEntry
    data class Row(val row: MessageRow) : ListEntry
}
