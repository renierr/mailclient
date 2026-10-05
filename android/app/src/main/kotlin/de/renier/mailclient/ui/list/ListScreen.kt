package de.renier.mailclient.ui.list

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.ui.state.MailState

// Step 1 message list: rows with sender, date, subject and snippet, star and
// attachment cues, "load older" tail. Sort, filter, search, selection and
// bulk arrive in Step 4; this screen proves paging, opening and the
// cache-first fill with real data.
@Composable
fun ListScreen(state: MailState, onOpenReader: (Long, Long, Int) -> Unit) {
    val folder = state.openFolder
    if (folder == null) {
        Text(
            "No folder open.",
            modifier = Modifier.padding(16.dp),
            style = MaterialTheme.typography.bodyMedium,
        )
        return
    }
    LazyColumn(modifier = Modifier.fillMaxSize()) {
        items(state.messages, key = { it.uid }) { m ->
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onOpenReader(state.activeAccountId, folder.id, m.uid) }
                    .padding(horizontal = 16.dp, vertical = 10.dp),
            ) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    val sender = m.fromName.ifEmpty { m.from }
                    Text(
                        // Text cues until the Step 4 icon set lands (no new
                        // dependency for placeholders — icons come from res/).
                        (if (m.starred) "★ " else "") + sender,
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = if (m.unread) FontWeight.Bold else null,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    Text(
                        m.date,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text(
                    (if (m.hasAttachments) "[att] " else "") + m.subject,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (m.snippet.isNotEmpty()) {
                    Text(
                        m.snippet,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        if (state.canLoadOlder) {
            item {
                TextButton(
                    onClick = { state.loadMore() },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Load older messages")
                }
            }
        }
    }
}
