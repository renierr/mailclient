package de.renier.mailclient.ui.folders

import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R

// The role glyph every folder list shows (sidebar, manager, move picker),
// the same mapping as Flutter's folderIcon. Decorative: the name beside it
// says what it is.
@Composable
fun FolderIcon(role: String, modifier: Modifier = Modifier) {
    val res = when (role) {
        "inbox" -> R.drawable.ic_inbox
        "sent" -> R.drawable.ic_send
        "drafts" -> R.drawable.ic_edit
        "trash" -> R.drawable.ic_delete
        "junk" -> R.drawable.ic_report
        "archive" -> R.drawable.ic_archive
        else -> R.drawable.ic_folder
    }
    Icon(
        painter = painterResource(res),
        contentDescription = null,
        tint = LocalContentColor.current,
        modifier = modifier.size(20.dp),
    )
}
