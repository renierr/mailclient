package de.renier.mailclient.ui.folders

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Badge
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.Avatar
import de.renier.mailclient.ui.common.PullToSync
import de.renier.mailclient.ui.state.Folder
import de.renier.mailclient.ui.state.MailState

// The sidebar pane: account chip (switch only — adding and managing live
// under Accounts), then this account's subscribed folders. A tap paints the
// cached rows at once and fills from the server behind them.
@Composable
fun FoldersScreen(
    state: MailState,
    onOpenFolder: () -> Unit,
    onAddAccount: () -> Unit,
) {
    Column(modifier = Modifier.fillMaxSize()) {
        if (state.accounts.isEmpty()) {
            Card(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("No accounts yet", style = MaterialTheme.typography.titleMedium)
                    Text(
                        "Add your first mail account to get started.",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    TextButton(onClick = onAddAccount) {
                        Text("Add account")
                    }
                }
            }
            return
        }

        AccountChip(state)
        HorizontalDivider()

        val folders = state.visibleFolders
        // The set lives in MailState, so navigating into a folder and back
        // keeps the tree as it was. Plain call, not memoized: reading the
        // snapshot set subscribes this composition, so a toggle recomposes
        // with fresh rows.
        val rows = collapseFolders(folders, state.expandedFolders)
        PullToSync(onSync = { state.syncNow() }) {
            LazyColumn(modifier = Modifier.fillMaxSize()) {
                if (folders.isEmpty()) {
                    item {
                        Box(modifier = Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) {
                            Text(
                                if (state.syncing || state.foldersBusy) "Syncing folders…"
                                else "No folders yet — pull down to sync, or manage folders.",
                                textAlign = TextAlign.Center,
                                color = MaterialTheme.colorScheme.outline,
                            )
                        }
                    }
                }
                items(rows, key = { it.folder.id }) { row ->
                    FolderRow(
                        folder = row.folder,
                        selected = row.folder.id == state.folderId,
                        unread = row.unread,
                        total = row.total,
                        collapsible = row.collapsible,
                        expanded = row.expanded,
                        onToggle = { state.toggleFolderExpanded(row.folder.id) },
                        onClick = {
                            state.openFolder(row.folder.id)
                            onOpenFolder()
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun AccountChip(state: MailState) {
    val active = state.activeAccount ?: return
    var expanded by remember { mutableStateOf(false) }
    val canSwitch = state.accounts.size > 1
    Box(modifier = Modifier.padding(8.dp)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .border(1.dp, MaterialTheme.colorScheme.outlineVariant, RoundedCornerShape(8.dp))
                .clickable(enabled = canSwitch) { expanded = true }
                .padding(horizontal = 10.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Avatar(
                initials = active.initials,
                avatarLight = active.avatarLight,
                avatarDark = active.avatarDark,
                size = 32.dp,
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    active.email,
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (canSwitch) {
                    Text(
                        "${state.accounts.size} accounts — switch",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            if (canSwitch) {
                Icon(
                    painter = painterResource(R.drawable.ic_expand_more),
                    contentDescription = "Switch account",
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
            for (a in state.accounts) {
                DropdownMenuItem(
                    text = {
                        Text(
                            a.email,
                            fontWeight = if (a.id == active.id) FontWeight.SemiBold else null,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    },
                    onClick = {
                        expanded = false
                        if (a.id != active.id) state.selectAccount(a.id)
                    },
                )
            }
        }
    }
}

@Composable
private fun FolderRow(
    folder: Folder,
    selected: Boolean,
    unread: Int,
    total: Int,
    collapsible: Boolean,
    expanded: Boolean,
    onToggle: () -> Unit,
    onClick: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(if (selected) scheme.secondaryContainer else scheme.surface)
            .clickable(onClick = onClick)
            // Hierarchy comes from the core's depth (IMAP path segments).
            .padding(start = (16 + folder.depth * 16).dp, end = 16.dp, top = 12.dp, bottom = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FolderIcon(folder.role)
        Text(
            folder.leaf,
            style = MaterialTheme.typography.bodyLarge,
            fontWeight = if (unread > 0) FontWeight.SemiBold else null,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (unread > 0) {
            Badge { Text(unread.toString()) }
        } else if (total > 0) {
            Text(
                total.toString(),
                style = MaterialTheme.typography.bodySmall,
                color = scheme.outline,
            )
        }
        // Collapse chevron last, so the label edge never moves whether a
        // row collapses or not. Rows with nothing to fold hold the same
        // slot, keeping badges aligned.
        if (collapsible) {
            IconButton(
                onClick = onToggle,
                modifier = Modifier.size(28.dp),
            ) {
                Icon(
                    painter = painterResource(
                        if (expanded) R.drawable.ic_expand_more else R.drawable.ic_chevron_right,
                    ),
                    contentDescription = if (expanded) "Collapse subfolders" else "Expand subfolders",
                    tint = scheme.onSurfaceVariant,
                )
            }
        } else {
            Spacer(modifier = Modifier.size(28.dp))
        }
    }
}

// One visible sidebar row: the folder plus its collapse state and the counts
// to paint (a collapsed parent aggregates its hidden children's counts, so
// no unread badge disappears with them).
private data class FolderRowState(
    val folder: Folder,
    val collapsible: Boolean,
    val expanded: Boolean,
    val unread: Int,
    val total: Int,
)

// The feed `leaf` is the last path segment; what precedes it (minus the
// single-char IMAP delimiter) is the parent. Depth 0 has none.
private fun parentPathOf(folder: Folder): String? {
    if (folder.depth <= 0) return null
    val cut = folder.path.length - folder.leaf.length - 1
    return if (cut > 0) folder.path.substring(0, cut) else null
}

// Fold the flat visible list into sidebar rows: `alwaysVisible` folders
// (top-level, well-known, and the inbox's direct children — the core's
// collapse rule) always show; custom subfolders show only while every
// ancestor up to the nearest always-visible one is expanded. A folder whose
// parent is not visible reads as a root, the way the old flat list showed
// it.
private fun collapseFolders(folders: List<Folder>, expanded: Set<Long>): List<FolderRowState> {
    val byPath = folders.associateBy { it.path }
    fun parentOf(folder: Folder): Folder? = byPath[parentPathOf(folder)]

    fun isShown(folder: Folder): Boolean {
        if (folder.depth <= 0 || folder.alwaysVisible) return true
        val parent = parentOf(folder) ?: return true
        return expanded.contains(parent.id) && isShown(parent)
    }

    fun isUnder(row: Folder, d: Folder): Boolean {
        var q: Folder? = d
        while (q != null) {
            if (q.id == row.id) return true
            q = parentOf(q)
        }
        return false
    }

    return folders.mapNotNull { folder ->
        if (!isShown(folder)) return@mapNotNull null
        // Collapsible only when the toggle hides something: a direct child
        // that folds away (custom role). INBOX, whose children all stay
        // visible, gets no chevron and stays inbox-only in counts.
        val collapsible = folders.any { parentOf(it)?.id == folder.id && !it.alwaysVisible }
        val open = expanded.contains(folder.id)
        var unread = folder.unread
        var total = folder.count
        if (collapsible && !open) {
            folders.forEach { d ->
                if (d.id != folder.id && !isShown(d) && isUnder(folder, d)) {
                    unread += d.unread
                    total += d.count
                }
            }
        }
        FolderRowState(folder, collapsible, open, unread, total)
    }
}
