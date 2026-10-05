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
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
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

// Message list: a header naming the folder (or the search), then rows with
// avatar + unread dot, sender/date, subject with star and attachment cues,
// snippet, and the "load older" tail. While searching, the rows are the
// hits, each carrying its own folder. Sort, filter, selection and bulk
// arrive in Step 4. Long-press opens the move picker for that row until
// Step 4d makes long-press start a selection (the bulk bar's Move then
// opens the same picker).
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
    val rows = if (searching) state.searchHits else state.messages
    // Account-wide hits name their folder; in-folder rows need not.
    val showFolder = searching && !state.searchFolderOnly
    val folderNames = remember(state.folders) { state.folders.associate { it.id to it.leaf } }

    var moving by remember { mutableStateOf<MessageRow?>(null) }
    moving?.let { m ->
        val from = if (m.folderId >= 0) m.folderId else folder?.id ?: -1
        MoveToDialog(
            folders = state.visibleFolders,
            currentFolderId = from,
            count = 1,
            subject = m.subject,
            onPick = { dest ->
                moving = null
                state.moveMessage(from, m.uid, dest.path)
            },
            onDismiss = { moving = null },
        )
    }

    PullToSync(syncing = state.syncing, onSync = { state.syncNow() }) {
        LazyColumn(modifier = Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 8.dp)) {
            item {
                ListHeader(
                    title = if (searching) "Search results" else folder?.leaf.orEmpty(),
                    subtitle = when {
                        searching && rows.isEmpty() -> "No matches"
                        searching -> "${rows.size} found" + if (state.searchFolderOnly && folder != null) " in ${folder.leaf}" else ""
                        folder != null && folder.unread > 0 -> "${folder.count} messages · ${folder.unread} unread"
                        folder != null -> "${folder.count} messages"
                        else -> ""
                    },
                )
            }
            items(rows, key = { "${it.folderId}:${it.uid}" }) { m ->
                val rowFolder = if (m.folderId >= 0) m.folderId else folder?.id ?: -1
                MessageItem(
                    m = m,
                    folderLabel = if (showFolder) folderNames[m.folderId] else null,
                    modifier = Modifier.combinedClickable(
                        onClick = { onOpenReader(state.activeAccountId, rowFolder, m.uid) },
                        onLongClick = { moving = m },
                        onLongClickLabel = "Move to folder",
                    ),
                )
            }
            if (!searching && state.canLoadOlder) {
                item {
                    Box(modifier = Modifier.fillMaxWidth().padding(16.dp), contentAlignment = Alignment.Center) {
                        OutlinedButton(onClick = { state.loadMore() }) { Text("Load older messages") }
                    }
                }
            }
        }
    }
}

@Composable
private fun ListHeader(title: String, subtitle: String) {
    Column(modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 8.dp)) {
        Text(title, style = MaterialTheme.typography.titleLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (subtitle.isNotEmpty()) {
            Text(
                subtitle,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun MessageItem(m: MessageRow, folderLabel: String?, modifier: Modifier) {
    val scheme = MaterialTheme.colorScheme
    val unreadDot = scheme.primary
    val ring = scheme.surface
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
    ) {
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
