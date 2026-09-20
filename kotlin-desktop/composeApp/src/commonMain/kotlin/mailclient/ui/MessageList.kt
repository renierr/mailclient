package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import mailclient.models.MessageRow
import mailclient.models.SearchHit

/**
 * Middle pane: toolbar search + message rows (QML MessageList.qml).
 * 3+ letters query the account-wide FTS index; shorter input filters
 * the current folder instantly. Selection is a UID (QML flaw F1).
 */
@Composable
fun MessageListPane(
    rows: List<MessageRow>,
    hits: List<SearchHit>,
    searching: Boolean,
    query: String,
    onQuery: (String) -> Unit,
    currentUid: Long?,
    currentFolderId: Long?,
    onSelect: (Long) -> Unit,
    onToggleStar: (Long, Boolean) -> Unit,
    onOpenSearchHit: (SearchHit) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier) {
        OutlinedTextField(
            value = query,
            onValueChange = onQuery,
            placeholder = { Text("Search (3+ letters: all folders)") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(8.dp),
        )
        if (searching) {
            Text(
                "${hits.size} result(s) across all folders",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 2.dp),
            )
            LazyColumn {
                items(hits, key = { "${it.folder_id}:${it.uid}" }) { h ->
                    Row(
                        Modifier.fillMaxWidth()
                            .clickable { onOpenSearchHit(h) }
                            .padding(horizontal = 12.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text(
                                h.subject,
                                fontWeight = if (h.unread) FontWeight.Bold else FontWeight.Normal,
                                fontSize = 14.sp,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Text(
                                "${h.from} · ${h.folder} · ${h.date}",
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            if (h.snippet.isNotBlank()) {
                                Text(
                                    h.snippet,
                                    fontSize = 12.sp,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            }
                        }
                    }
                }
                if (hits.isEmpty()) {
                    item {
                        Text(
                            "No matches.",
                            fontSize = 12.sp,
                            modifier = Modifier.padding(12.dp),
                        )
                    }
                }
            }
        } else {
            LazyColumn {
                items(rows, key = { it.uid }) { m ->
                    val selected = m.uid == currentUid
                    Column(
                        Modifier.fillMaxWidth()
                            .clickable { onSelect(m.uid) }
                            .background(
                                if (selected) MaterialTheme.colorScheme.primary.copy(alpha = 0.12f)
                                else androidx.compose.ui.graphics.Color.Transparent,
                            )
                            .padding(horizontal = 12.dp, vertical = 8.dp),
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            if (m.unread) {
                                Text("● ", color = UnreadAccent, fontSize = 12.sp)
                            }
                            Text(
                                m.subject,
                                fontWeight = if (m.unread) FontWeight.Bold else FontWeight.Normal,
                                fontSize = 14.sp,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            if (m.has_attachments) Text(" 📎", fontSize = 12.sp)
                            Text(
                                if (m.starred) " ★" else " ☆",
                                color = if (m.starred) StarOn else StarOff,
                                fontSize = 14.sp,
                                modifier = Modifier.clickable { onToggleStar(m.uid, m.starred) }
                                    .padding(start = 4.dp),
                            )
                        }
                        Row(
                            Modifier.fillMaxWidth(),
                            horizontalArrangement = Arrangement.SpaceBetween,
                        ) {
                            Text(
                                m.from,
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            Text(
                                m.date,
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        if (m.snippet.isNotBlank()) {
                            Text(
                                m.snippet,
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
                if (rows.isEmpty()) {
                    item {
                        Text(
                            if (currentFolderId == null) "No account — add one in the QML app first."
                            else "Empty folder.",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(12.dp),
                        )
                    }
                }
            }
        }
    }
}
