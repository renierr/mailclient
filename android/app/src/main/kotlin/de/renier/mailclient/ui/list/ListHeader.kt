package de.renier.mailclient.ui.list

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.clearListFilters
import de.renier.mailclient.ui.state.exitSelectionMode
import de.renier.mailclient.ui.state.hasListFilter
import de.renier.mailclient.ui.state.invertSelection
import de.renier.mailclient.ui.state.selectAllVisible
import de.renier.mailclient.ui.state.selectStarredVisible
import de.renier.mailclient.ui.state.selectUnreadVisible
import de.renier.mailclient.ui.state.visibleRows

// The list's top chrome: title row with sort / filter / select menus, the
// active-filter bar and the find-similar chip.

@Composable
internal fun ListHeaderRow(
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
internal fun FilterBar(state: MailState) {
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

// Qt's dismissable "Similar to: …" chip: leaving similar mode returns to
// the folder (a tablet has no other way out while the field is empty).
@Composable
internal fun SimilarBar(label: String, onClose: () -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            label,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.primary,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        IconButton(onClick = onClose) {
            Icon(painterResource(R.drawable.ic_close), "Close similar messages")
        }
    }
}
