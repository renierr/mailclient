package de.renier.mailclient.ui.folders

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Badge
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
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
                items(folders, key = { it.id }) { folder ->
                    FolderRow(
                        folder = folder,
                        selected = folder.id == state.folderId,
                        onClick = {
                            state.openFolder(folder.id)
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
private fun FolderRow(folder: Folder, selected: Boolean, onClick: () -> Unit) {
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
            fontWeight = if (folder.unread > 0) FontWeight.SemiBold else null,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (folder.unread > 0) {
            Badge { Text(folder.unread.toString()) }
        } else if (folder.count > 0) {
            Text(
                folder.count.toString(),
                style = MaterialTheme.typography.bodySmall,
                color = scheme.outline,
            )
        }
    }
}
