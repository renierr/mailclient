package de.renier.mailclient.ui.list

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import de.renier.mailclient.ui.common.Avatar
import de.renier.mailclient.ui.state.MailState

// Step 1 message list: avatar + unread dot, sender/date, subject, snippet,
// star and attachment cues, "load older" tail. Sort, filter, search,
// selection and bulk arrive in Step 4.
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
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable { onOpenReader(state.activeAccountId, folder.id, m.uid) }
                    .padding(horizontal = 16.dp, vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Box {
                    Avatar(
                        initials = m.initials,
                        avatarLight = m.avatarLight,
                        avatarDark = m.avatarDark,
                    )
                    if (m.unread) {
                        Canvas(
                            modifier = Modifier
                                .size(14.dp)
                                .align(Alignment.TopEnd)
                                .offset(x = 3.dp, y = (-3).dp),
                        ) {
                            drawCircle(color = Color.White, radius = size.minDimension / 2)
                            drawCircle(
                                color = Color(0xFF1A73E8),
                                radius = size.minDimension / 2 - 3.dp.toPx() / 2,
                            )
                        }
                    }
                }
                Column(modifier = Modifier.weight(1f)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        val sender = m.fromName.ifEmpty { m.from }
                        Text(
                            sender,
                            style = MaterialTheme.typography.bodyLarge,
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
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        if (m.starred) {
                            Text(
                                "★",
                                color = MaterialTheme.colorScheme.primary,
                                fontSize = 14.sp,
                            )
                        }
                        Text(
                            m.subject,
                            style = MaterialTheme.typography.bodyMedium,
                            fontWeight = if (m.unread) FontWeight.SemiBold else null,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                        if (m.hasAttachments) {
                            Text(
                                "[att]",
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    if (m.snippet.isNotEmpty()) {
                        Text(
                            m.snippet,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
            HorizontalDivider(modifier = Modifier.padding(start = 68.dp))
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
