package de.renier.mailclient.ui.folders

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.ui.state.Folder

// Move picker (Flutter MoveToDialog): subscribed folders of the account,
// indented, the folder the mail is in disabled. Pure choice, so it stays a
// dialog. [count] > 1 names the selection, else [subject] the one message.
@Composable
fun MoveToDialog(
    folders: List<Folder>,
    currentFolderId: Long,
    count: Int,
    subject: String?,
    onPick: (Folder) -> Unit,
    onDismiss: () -> Unit,
) {
    val title = when {
        count > 1 -> "Move $count messages to:"
        !subject.isNullOrEmpty() -> "Move “$subject” to:"
        else -> "Move to:"
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title, maxLines = 2, overflow = TextOverflow.Ellipsis) },
        text = {
            if (folders.isEmpty()) {
                Text("No other folders available.")
            } else {
                LazyColumn(modifier = Modifier.heightIn(max = 420.dp)) {
                    items(folders, key = { it.id }) { f ->
                        val current = f.id == currentFolderId
                        val tint = if (current) MaterialTheme.colorScheme.outline
                        else MaterialTheme.colorScheme.onSurface
                        CompositionLocalProvider(LocalContentColor provides tint) {
                            Row(
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .clickable(enabled = !current) { onPick(f) }
                                    .padding(start = (f.depth * 16).dp, top = 12.dp, bottom = 12.dp),
                                horizontalArrangement = Arrangement.spacedBy(12.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                FolderIcon(f.role)
                                Text(
                                    f.leaf,
                                    color = tint,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    modifier = Modifier.weight(1f),
                                )
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
