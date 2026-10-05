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
import androidx.compose.material3.Badge
import androidx.compose.material3.Card
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import de.renier.mailclient.ui.state.MailState

// Step 1 folders: account switcher + subscribed folder tree with unread
// pills. Folder management (create/subscribe/refresh) arrives in Step 3;
// this screen proves selection, switching and navigation with real data.
@OptIn(ExperimentalMaterial3Api::class)
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
        ExposedDropdownMenuBox(
            expanded = expanded && state.accounts.size > 1,
            onExpandedChange = { expanded = it },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        ) {
            OutlinedTextField(
                value = active?.email ?: "",
                onValueChange = {},
                readOnly = true,
                label = { Text("Account") },
                trailingIcon = {
                    if (state.accounts.size > 1) ExposedDropdownMenuDefaults.TrailingIcon(expanded)
                },
                modifier = Modifier.fillMaxWidth().menuAnchor(
                    ExposedDropdownMenuAnchorType.PrimaryNotEditable,
                    enabled = state.accounts.size > 1,
                ),
            )
            ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
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
                            start = (16 + folder.depth * 16).dp,
                            end = 16.dp,
                            top = 12.dp,
                            bottom = 12.dp,
                        ),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
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
            }
        }
    }
}
