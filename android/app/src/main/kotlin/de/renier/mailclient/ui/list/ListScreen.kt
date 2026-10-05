package de.renier.mailclient.ui.list

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.Avatar
import de.renier.mailclient.ui.common.PullToSync
import de.renier.mailclient.ui.folders.MoveToDialog
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.MessageRow
import de.renier.mailclient.ui.theme.starColor
import kotlinx.coroutines.launch

// Message list: a pinned header (folder or search title, sort, filter,
// selection), an active-filter bar, rows with avatar + unread dot (a
// checkbox in selection mode), sender/date, subject with star and
// attachment cues, snippet, and the "load older" tail. Jump buttons float
// over long lists; the bulk bar docks at the bottom while selected.
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ListScreen(state: MailState, onOpenReader: (Long, Long, Int) -> Unit) {
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
    val entries: List<ListEntry> = remember(rows, searching, showFolder) {
        if (!searching || !showFolder) {
            rows.map { ListEntry.Row(it) }
        } else {
            buildList {
                var lastFolder = Long.MIN_VALUE
                for (m in rows) {
                    val fid = if (m.folderId >= 0) m.folderId else folder?.id ?: -1
                    if (fid != lastFolder) {
                        lastFolder = fid
                        add(ListEntry.Header(fid, folderNames[fid] ?: "?"))
                    }
                    add(ListEntry.Row(m))
                }
            }
        }
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
    val jumpScope = rememberCoroutineScope()

    var dateDialog by remember { mutableStateOf(false) }
    var bulkMove by remember { mutableStateOf(false) }
    var confirmTrash by remember { mutableStateOf(false) }
    var confirmPurge by remember { mutableStateOf(false) }

    if (bulkMove) {
        // One shared folder when the selection sits in it, else none
        // disabled — search selections can span folders.
        val singleFolder = if (searching) {
            rows.filter { state.selectedKeys.contains(state.selectionKey(it)) }
                .map { if (it.folderId >= 0) it.folderId else state.folderId }
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
    if (confirmTrash) {
        // Bulk delete always confirms — even where delete is permanent
        // (no undo there) and even with the confirm preference off. The
        // preference only governs single-message reader deletes.
        val permanent = state.selectionDeleteIsPermanent
        DeleteConfirmDialog(
            title = if (permanent) "Delete permanently?" else "Move to Trash?",
            text = if (permanent) {
                "Permanently delete ${state.selectionCount} messages? This cannot be undone."
            } else {
                "Move ${state.selectionCount} messages to Trash? You can undo this."
            },
            confirmLabel = if (permanent) "Delete" else "Move to Trash",
            onConfirm = {
                confirmTrash = false
                state.bulkTrash()
            },
            onDismiss = { confirmTrash = false },
        )
    }
    if (confirmPurge) {
        DeleteConfirmDialog(
            title = "Delete permanently?",
            text = "Permanently delete ${state.selectionCount} messages? This cannot be undone.",
            confirmLabel = "Delete",
            onConfirm = {
                confirmPurge = false
                state.bulkPurge()
            },
            onDismiss = { confirmPurge = false },
        )
    }
    if (dateDialog) {
        DateRangeDialog(state) { dateDialog = false }
    }

    PullToSync(syncing = state.syncing, onSync = { state.syncNow() }) {
        Column(modifier = Modifier.fillMaxSize()) {
            ListHeaderRow(
                state = state,
                title = if (searching) {
                    state.similarLabel ?: "Search results"
                } else {
                    folder?.leaf.orEmpty()
                },
                subtitle = when {
                    searching && total == 0 && state.serverSearchPending -> "Searching server…"
                    searching && total == 0 -> "No matches"
                    searching -> "${rows.size} found" +
                        if (state.searchFolderOnly && folder != null) " in ${folder.leaf}" else ""
                    folder != null && state.hasListFilter -> "${rows.size} of $total shown"
                    folder != null && folder.unread > 0 -> "${folder.count} messages · ${folder.unread} unread"
                    folder != null -> "${folder.count} messages"
                    else -> ""
                },
                onCustomRange = { dateDialog = true },
            )
            if (state.hasListFilter) {
                FilterBar(state)
            }
            Box(modifier = Modifier.weight(1f)) {
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxSize(),
                    // Room for the floating jump buttons over the last row.
                    contentPadding = PaddingValues(bottom = 96.dp),
                ) {
                    items(entries, key = { if (it is ListEntry.Header) "h:${it.folderId}" else state.selectionKey((it as ListEntry.Row).row) }) { entry ->
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
                        val rowFolder = if (m.folderId >= 0) m.folderId else folder?.id ?: -1
                        MessageItem(
                            m = m,
                            folderLabel = if (showFolder) folderNames[m.folderId] else null,
                            selected = if (state.selectionMode) key in state.selectedKeys else null,
                            modifier = Modifier.combinedClickable(
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
                    // Always-on footer (Qt loadOlderBar, Flutter LoadOlderTile):
                    // the server status stays visible even with nothing left
                    // to load, so the list never ends in silence.
                    if (!searching && state.olderState.isNotEmpty() && state.olderState != "empty") {
                        item {
                            Column(
                                modifier = Modifier.fillMaxWidth().padding(16.dp),
                                horizontalAlignment = Alignment.CenterHorizontally,
                            ) {
                                Text(
                                    state.olderLabel +
                                        if (state.hasListFilter) " · filters cover loaded mail only" else "",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                                if (state.canLoadOlder) {
                                    OutlinedButton(
                                        onClick = { state.loadMore() },
                                        enabled = !state.syncing,
                                        modifier = Modifier.padding(top = 8.dp),
                                    ) {
                                        Text("Load older messages")
                                    }
                                }
                            }
                        }
                    }
                }
                // Jump to top / bottom (Qt ScrollJumpButtons, Flutter
                // ScrollJumpOverlay): only over long lists, each end only
                // while it is off-screen.
                val long = listState.layoutInfo.totalItemsCount > 12
                if (long && (listState.canScrollBackward || listState.canScrollForward)) {
                    Column(
                        modifier = Modifier
                            .align(Alignment.BottomEnd)
                            .padding(end = 16.dp, bottom = 16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        if (listState.canScrollBackward) {
                            FilledTonalIconButton(
                                onClick = { jumpScope.launch { listState.scrollToItem(0) } },
                            ) {
                                Icon(
                                    painter = painterResource(R.drawable.ic_arrow_up),
                                    contentDescription = "Jump to top",
                                )
                            }
                        }
                        if (listState.canScrollForward) {
                            FilledTonalIconButton(
                                onClick = {
                                    jumpScope.launch {
                                        listState.scrollToItem(listState.layoutInfo.totalItemsCount - 1)
                                    }
                                },
                            ) {
                                Icon(
                                    painter = painterResource(R.drawable.ic_arrow_down),
                                    contentDescription = "Jump to bottom",
                                )
                            }
                        }
                    }
                }
            }
            if (state.selectionMode && state.selectionCount > 0) {
                ListBulkBar(
                    state = state,
                    onMove = { bulkMove = true },
                    onTrash = { confirmTrash = true },
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

@Composable
private fun ListHeaderRow(
    state: MailState,
    title: String,
    subtitle: String,
    onCustomRange: () -> Unit,
) {
    // Each menu renders inside a Box around its own button, so it anchors
    // to the button instead of floating at the screen edge.
    var sortOpen by remember { mutableStateOf(false) }
    var filterOpen by remember { mutableStateOf(false) }
    var selectOpen by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, top = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (state.selectionMode) {
            val visible = state.visibleRows()
            val all = visible.isNotEmpty() && state.selectedKeys.size == visible.size
            Checkbox(
                checked = all,
                onCheckedChange = {
                    if (it) state.selectAllVisible() else state.exitSelectionMode()
                },
            )
        }
        Column(modifier = Modifier.weight(1f).padding(horizontal = 12.dp)) {
            Text(title, style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            if (subtitle.isNotEmpty()) {
                Text(
                    subtitle,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (state.selectionMode) {
            Box {
                IconButton(onClick = { selectOpen = true }) {
                    Icon(
                        painter = painterResource(R.drawable.ic_expand_more),
                        contentDescription = "Select messages",
                    )
                }
                if (selectOpen) {
                    DropdownMenu(expanded = true, onDismissRequest = { selectOpen = false }) {
                        DropdownMenuItem(
                            text = { Text("Select all") },
                            onClick = { selectOpen = false; state.selectAllVisible() },
                        )
                        DropdownMenuItem(
                            text = { Text("Select unread") },
                            onClick = { selectOpen = false; state.selectUnreadVisible() },
                        )
                        DropdownMenuItem(
                            text = { Text("Select starred") },
                            onClick = { selectOpen = false; state.selectStarredVisible() },
                        )
                        DropdownMenuItem(
                            text = { Text("Invert selection") },
                            onClick = { selectOpen = false; state.invertSelection() },
                        )
                    }
                }
            }
            IconButton(onClick = { state.exitSelectionMode() }) {
                Icon(
                    painter = painterResource(R.drawable.ic_close),
                    contentDescription = "Leave selection",
                )
            }
        } else {
            if (!state.searchActive) {
                Box {
                    IconButton(onClick = { sortOpen = true }) {
                        Icon(
                            painter = painterResource(R.drawable.ic_sort),
                            contentDescription = "Sort: ${sortShortLabel(state)}",
                        )
                    }
                    if (sortOpen) SortMenu(state) { sortOpen = false }
                }
            }
            Box {
                IconButton(onClick = { filterOpen = true }) {
                    Icon(
                        painter = painterResource(R.drawable.ic_filter),
                        contentDescription = "Filter messages",
                        tint = if (state.hasListFilter) {
                            MaterialTheme.colorScheme.primary
                        } else {
                            MaterialTheme.colorScheme.onSurfaceVariant
                        },
                    )
                }
                if (filterOpen) {
                    FilterMenu(
                        state,
                        onCustomRange = { filterOpen = false; onCustomRange() },
                    ) { filterOpen = false }
                }
            }
        }
    }
}

@Composable
private fun FilterBar(state: MailState) {
    val parts = buildList {
        if (state.filterUnread) add("Unread")
        if (state.filterStarred) add("Starred")
        if (state.filterAttachments) add("Attachments")
        if (state.dateFilterLabel.isNotEmpty()) add(state.dateFilterLabel)
    }
    Row(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            parts.joinToString(" · "),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.primary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        TextButton(onClick = { state.clearListFilters() }) { Text("Clear") }
    }
}

@Composable
private fun MessageItem(
    m: MessageRow,
    folderLabel: String?,
    selected: Boolean?,
    modifier: Modifier,
    onToggle: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val unreadDot = scheme.primary
    val ring = scheme.surface
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        if (selected != null) {
            Checkbox(checked = selected, onCheckedChange = { onToggle() })
        } else {
            Box {
                Avatar(initials = m.initials, avatarLight = m.avatarLight, avatarDark = m.avatarDark)
                if (m.unread) {
                    Canvas(
                        modifier = Modifier
                            .size(14.dp)
                            .align(Alignment.TopEnd)
                            .offset(x = 3.dp, y = (-3).dp),
                    ) {
                        drawCircle(color = ring, radius = size.minDimension / 2)
                        drawCircle(color = unreadDot, radius = size.minDimension / 2 - 3.dp.toPx() / 2)
                    }
                }
            }
        }
        Column(modifier = Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    m.fromName.ifEmpty { m.from },
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = if (m.unread) FontWeight.Bold else null,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    m.date,
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = if (m.unread) FontWeight.Bold else null,
                    color = if (m.unread) scheme.primary else scheme.onSurfaceVariant,
                )
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    m.subject,
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = if (m.unread) FontWeight.SemiBold else null,
                    color = scheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (m.hasAttachments) {
                    Icon(
                        painter = painterResource(R.drawable.ic_attach),
                        contentDescription = "Has attachments",
                        tint = scheme.onSurfaceVariant,
                        modifier = Modifier.size(16.dp),
                    )
                }
                if (m.starred) {
                    Icon(
                        painter = painterResource(R.drawable.ic_star),
                        contentDescription = "Starred",
                        tint = starColor(true),
                        modifier = Modifier.size(16.dp),
                    )
                }
            }
            if (m.snippet.isNotEmpty()) {
                Text(
                    m.snippet,
                    style = MaterialTheme.typography.bodyMedium,
                    color = scheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            folderLabel?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.labelSmall,
                    color = scheme.primary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}
