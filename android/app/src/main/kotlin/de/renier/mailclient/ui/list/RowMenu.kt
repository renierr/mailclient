package de.renier.mailclient.ui.list

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.state.MessageRow

/** What a row's ⋮ menu can do; Qt's and Flutter's row actions. */
enum class RowAction { Read, Star, Archive, Move, Trash, Purge, Similar, SaveEml }

/**
 * A row's ⋮ button and its menu. Small, on the subject line under the date:
 * the whole row stays the tap target for opening, so the button only
 * needs to be findable. [permanent]: delete in this folder destroys.
 */
@Composable
fun RowMenuButton(m: MessageRow, permanent: Boolean, onAction: (RowAction) -> Unit) {
    var open by remember { mutableStateOf(false) }
    fun pick(a: RowAction) {
        open = false
        onAction(a)
    }
    Box {
        IconButton(onClick = { open = true }, modifier = Modifier.size(32.dp)) {
            Icon(
                painterResource(R.drawable.ic_more_vert),
                "Message actions",
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(18.dp),
            )
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            Item(if (m.unread) "Mark as read" else "Mark as unread", R.drawable.ic_mail) { pick(RowAction.Read) }
            Item(if (m.starred) "Remove star" else "Star", if (m.starred) R.drawable.ic_star else R.drawable.ic_star_border) {
                pick(RowAction.Star)
            }
            Item("Archive", R.drawable.ic_archive) { pick(RowAction.Archive) }
            Item("Move to…", R.drawable.ic_folder) { pick(RowAction.Move) }
            if (!permanent) Item("Move to Trash", R.drawable.ic_delete) { pick(RowAction.Trash) }
            Item("Delete permanently…", R.drawable.ic_delete_forever) { pick(RowAction.Purge) }
            HorizontalDivider()
            Item("Find similar", R.drawable.ic_search) { pick(RowAction.Similar) }
            Item("Save as .eml…", R.drawable.ic_download) { pick(RowAction.SaveEml) }
        }
    }
}

@Composable
private fun Item(text: String, icon: Int, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(text) },
        leadingIcon = { Icon(painterResource(icon), null, Modifier.size(20.dp)) },
        onClick = onClick,
    )
}
