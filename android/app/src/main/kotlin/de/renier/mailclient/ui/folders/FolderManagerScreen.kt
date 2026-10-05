package de.renier.mailclient.ui.folders

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.state.Folder
import de.renier.mailclient.ui.state.MailState

// The IMAP folder manager (Flutter FolderManagerDialog / Qt Folders.qml):
// create folders (`/` nests), hide them from the sidebar, refresh the server
// list, jump to one. Hiding is display-only — a hidden folder keeps its
// cache and still quick-syncs. A full page on every width: it has a text
// field, and header plus rows scroll as one list so the keyboard never
// squeezes a fixed header.
@Composable
fun FolderManagerScreen(state: MailState, onOpenFolder: () -> Unit) {
    var name by rememberSaveable { mutableStateOf("") }
    var creating by rememberSaveable { mutableStateOf(false) }
    // Why the last create failed, under the field: the status line is far
    // away on a phone.
    var error by rememberSaveable { mutableStateOf<String?>(null) }

    fun create() {
        val path = name.trim()
        if (path.isEmpty() || creating) return
        creating = true
        error = null
        state.createFolder(path) { ok, status ->
            creating = false
            // The name stays on failure, so a typo can be fixed and retried.
            if (ok) name = "" else error = status.ifEmpty { "Could not create the folder" }
        }
    }

    LazyColumn(modifier = Modifier.fillMaxSize()) {
        item {
            Column(
                modifier = Modifier.fillMaxWidth().padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("New folder name (/ for subfolders)") },
                    singleLine = true,
                    isError = error != null,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    keyboardActions = KeyboardActions(onDone = { create() }),
                    modifier = Modifier.fillMaxWidth(),
                )
                error?.let {
                    Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                }
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    OutlinedButton(
                        onClick = { state.refreshFolderList() },
                        enabled = !state.foldersBusy,
                    ) {
                        if (state.foldersBusy && !creating) {
                            CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                        } else {
                            Icon(
                                painter = painterResource(R.drawable.ic_refresh),
                                contentDescription = null,
                                modifier = Modifier.size(16.dp),
                            )
                        }
                        Text("Refresh", modifier = Modifier.padding(start = 6.dp))
                    }
                    Button(
                        onClick = { create() },
                        enabled = name.isNotBlank() && !creating && !state.foldersBusy,
                    ) {
                        if (creating) {
                            CircularProgressIndicator(modifier = Modifier.size(16.dp), strokeWidth = 2.dp)
                        } else {
                            Text("Create")
                        }
                    }
                }
                Text(
                    "Uncheck to hide a folder from the sidebar.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            HorizontalDivider()
        }
        if (state.folders.isEmpty()) {
            item {
                Box(modifier = Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) {
                    Text("No folders yet — press Refresh.", color = MaterialTheme.colorScheme.outline)
                }
            }
        }
        items(state.folders, key = { it.id }) { folder ->
            ManagerRow(
                folder = folder,
                current = folder.id == state.folderId,
                onSubscribed = { state.setFolderSubscribed(folder.id, it) },
                onOpen = {
                    state.openFolder(folder.id)
                    onOpenFolder()
                },
            )
            HorizontalDivider()
        }
    }
}

@Composable
private fun ManagerRow(
    folder: Folder,
    current: Boolean,
    onSubscribed: (Boolean) -> Unit,
    onOpen: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = !current, onClick = onOpen)
            .padding(start = 4.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(checked = folder.subscribed, onCheckedChange = onSubscribed)
        FolderIcon(folder.role)
        Column(modifier = Modifier.weight(1f).padding(start = 10.dp)) {
            Text(
                folder.path,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                if (current) "${folder.count} · ${folder.unread} unread · open"
                else "${folder.count} · ${folder.unread} unread",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (!current) {
            Icon(
                painter = painterResource(R.drawable.ic_chevron_right),
                contentDescription = "Open folder",
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}
