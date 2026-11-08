package de.renier.mailclient.ui.list

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.MessageRow
import de.renier.mailclient.ui.state.syncNow
import de.renier.mailclient.ui.common.PullToSync

/** Local inbox list. Opening a row lands in its real account and folder. */
@Composable
fun CombinedInbox(state: MailState, onOpen: (MessageRow) -> Unit) {
    Column(Modifier.fillMaxSize()) {
        Text("Combined Inbox", style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(16.dp))
        Text(
            "Cached inboxes · Search is local. Open a message to use its account.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
        )
        if (state.combinedMessages.isEmpty()) {
            Text("No cached messages", modifier = Modifier.padding(16.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        PullToSync(onSync = { state.syncNow() }, modifier = Modifier.weight(1f)) {
            LazyColumn(Modifier.fillMaxSize()) {
                items(state.combinedMessages, key = { "${it.folderId}:${it.uid}" }) { row ->
                    val account = row.accountEmail.ifEmpty { state.accounts.firstOrNull { it.id == row.accountId }?.email.orEmpty() }
                    Column(Modifier.fillMaxWidth().clickable { onOpen(row) }.padding(horizontal = 16.dp, vertical = 12.dp)) {
                        Text(row.fromName.ifEmpty { row.from }, fontWeight = if (row.unread) FontWeight.Bold else FontWeight.Normal, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(row.subject, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text("$account · ${row.date}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    HorizontalDivider()
                }
            }
        }
    }
}
