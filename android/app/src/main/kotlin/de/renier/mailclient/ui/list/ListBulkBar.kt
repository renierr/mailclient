package de.renier.mailclient.ui.list

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.theme.starColor

// Bulk action bar (Qt BulkActionBar, Flutter BulkActionBar): the selection's
// count plus every action as an icon+text button in one scrollable row —
// no overflow menu. The common actions lead (Archive, Trash). Where the
// core says deleting destroys (Trash itself, a folder without Trash), the
// Trash button becomes "Delete" and the separate purge button goes, since
// both would do the same. Move opens the picker's bulk form; trash and
// purge confirm in the list screen first. The select helpers (all / unread
// / starred / invert) live only in the header's select menu, not here.
@Composable
fun ListBulkBar(
    state: MailState,
    onMove: () -> Unit,
    onTrash: () -> Unit,
    onPurge: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Surface(
        tonalElevation = 3.dp,
        shadowElevation = 4.dp,
        modifier = modifier.fillMaxWidth(),
    ) {
        Row(
            modifier = Modifier
                .horizontalScroll(rememberScrollState())
                .padding(horizontal = 4.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "${state.selectionCount}",
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(horizontal = 12.dp),
            )
            BulkButton("Clear", R.drawable.ic_close, "Clear selection") {
                state.exitSelectionMode()
            }
            val permanent = state.selectionDeletePrompt.permanent
            BulkButton("Archive", R.drawable.ic_archive, "Archive") {
                state.bulkArchive()
            }
            if (permanent) {
                BulkButton("Delete", R.drawable.ic_delete_forever, "Delete permanently", onClick = onTrash)
            } else {
                BulkButton("Trash", R.drawable.ic_delete, "Move to Trash", onClick = onTrash)
            }
            BulkButton("Move", R.drawable.ic_folder, "Move to folder", onClick = onMove)
            BulkButton("Read", R.drawable.ic_check, "Mark read") {
                state.bulkMarkRead(true)
            }
            BulkButton("Unread", R.drawable.ic_mail, "Mark unread") {
                state.bulkMarkRead(false)
            }
            BulkButton(
                if (state.selectionAllStarred) "Unstar" else "Star",
                if (state.selectionAllStarred) R.drawable.ic_star else R.drawable.ic_star_border,
                if (state.selectionAllStarred) "Unstar" else "Star",
                tintStar = !state.selectionAllStarred,
            ) {
                state.bulkStar(!state.selectionAllStarred)
            }
            if (!permanent) {
                BulkButton("Delete…", R.drawable.ic_delete_forever, "Delete permanently", onClick = onPurge)
            }
        }
    }
}

@Composable
private fun BulkButton(
    label: String,
    icon: Int,
    description: String,
    tintStar: Boolean = false,
    onClick: () -> Unit,
) {
    TextButton(onClick = onClick) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                painter = painterResource(icon),
                contentDescription = description,
                tint = if (tintStar) {
                    starColor(true)
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
                modifier = Modifier.size(18.dp),
            )
            Text(label)
        }
    }
}
