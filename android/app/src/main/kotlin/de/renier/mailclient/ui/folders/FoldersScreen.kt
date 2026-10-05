package de.renier.mailclient.ui.folders

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Badge
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import de.renier.mailclient.ui.common.Avatar
import de.renier.mailclient.ui.state.MailState

// Step 1 folders: account switcher + subscribed folder tree with unread
// pills. Folder management (create/subscribe/refresh) arrives in Step 3;
// this screen proves selection, switching and navigation with real data.
@Composable
fun FoldersScreen(state: MailState, onOpenFolder: () -> Unit) {
    Column(modifier = Modifier.fillMaxSize()) {
        if (state.accounts.isEmpty()) {
            Card(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("No accounts yet", style = MaterialTheme.typography.titleMedium)
                    Text(
                        "Add your first mail account to get started.",
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    TextButton(onClick = { state.info("Account setup arrives in Step 2") }) {
                        Text("Add account")
                    }
                }
            }
            return
        }

        var expanded by remember { mutableStateOf(false) }
        val active = state.activeAccount
        if (active != null) {
            Row(
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Avatar(
                    initials = active.initials,
                    avatarLight = active.avatarLight,
                    avatarDark = active.avatarDark,
                    size = 44.dp,
                )
                Column(modifier = Modifier.weight(1f)) {
                    Text(active.name, style = MaterialTheme.typography.titleMedium)
                    Text(
                        active.email,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                if (state.accounts.size > 1) {
                    TextButton(onClick = { expanded = true }) { Text("Switch") }
                    DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                        for (a in state.accounts) {
                            DropdownMenuItem(
                                text = { Text(a.email) },
                                onClick = {
                                    expanded = false
                                    state.selectAccount(a.id)
                                },
                            )
                        }
                    }
                }
            }
            HorizontalDivider()
        }

        LazyColumn(modifier = Modifier.fillMaxSize()) {
            items(state.folders, key = { it.id }) { folder ->
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable {
                            state.openFolder(folder.id)
                            onOpenFolder()
                        }
                        .padding(
                            start = (16 + folder.depth * 20).dp,
                            end = 16.dp,
                            top = 13.dp,
                            bottom = 13.dp,
                        ),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        folder.leaf.ifEmpty { folder.path },
                        style = MaterialTheme.typography.bodyLarge,
                        fontWeight = if (folder.unread > 0) FontWeight.Bold else null,
                        modifier = Modifier.weight(1f),
                    )
                    if (folder.unread > 0) {
                        Badge { Text(folder.unread.toString()) }
                    } else if (folder.count > 0) {
                        Text(
                            folder.count.toString(),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                HorizontalDivider(modifier = Modifier.padding(start = (16 + folder.depth * 20).dp))
            }
        }
    }
}
