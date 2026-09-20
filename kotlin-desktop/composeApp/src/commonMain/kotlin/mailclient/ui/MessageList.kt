package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import mailclient.models.MessageRow
import mailclient.models.SearchHit

/**
 * Middle pane: header bar with folder name, count, sort dropdown, and message rows.
 * (Search is located in the top toolbar, matching QML Main.qml).
 */
@Composable
fun MessageListPane(
    folderName: String,
    rows: List<MessageRow>,
    hits: List<SearchHit>,
    searching: Boolean,
    currentUid: Long?,
    currentFolderId: Long?,
    sortField: String,
    sortDescending: Boolean,
    onSortChange: (field: String, descending: Boolean) -> Unit,
    onSelect: (Long) -> Unit,
    onToggleStar: (Long, Boolean) -> Unit,
    onOpenSearchHit: (SearchHit) -> Unit,
    modifier: Modifier = Modifier,
) {
    var sortMenuExpanded by remember { mutableStateOf(false) }

    Column(modifier.background(MaterialTheme.colorScheme.background)) {
        // --- Header Bar (Folder name, count, sort) ---
        Row(
            Modifier
                .fillMaxWidth()
                .height(40.dp)
                .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f))
                .padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                modifier = Modifier.weight(1f),
            ) {
                Text(
                    text = if (searching) "Search results" else folderName.ifBlank { "Messages" },
                    fontWeight = FontWeight.Bold,
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    text = if (searching) "${hits.size}" else "${rows.size}",
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            // Sort Menu
            Box {
                val arrow = if (sortDescending) "↓" else "↑"
                val sortLabel = when (sortField) {
                    "from" -> "From $arrow"
                    "subject" -> "Subject $arrow"
                    else -> "Date $arrow"
                }

                Row(
                    Modifier
                        .clip(RoundedCornerShape(4.dp))
                        .clickable { sortMenuExpanded = true }
                        .padding(horizontal = 6.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text("⇅", fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Text(sortLabel, fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }

                DropdownMenu(
                    expanded = sortMenuExpanded,
                    onDismissRequest = { sortMenuExpanded = false },
                ) {
                    DropdownMenuItem(
                        text = { Text("Date (Newest first)") },
                        onClick = {
                            sortMenuExpanded = false
                            onSortChange("date", true)
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Date (Oldest first)") },
                        onClick = {
                            sortMenuExpanded = false
                            onSortChange("date", false)
                        },
                    )
                    HorizontalDivider(Modifier.padding(vertical = 4.dp))
                    DropdownMenuItem(
                        text = { Text("From (A–Z)") },
                        onClick = {
                            sortMenuExpanded = false
                            onSortChange("from", false)
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("From (Z–A)") },
                        onClick = {
                            sortMenuExpanded = false
                            onSortChange("from", true)
                        },
                    )
                    HorizontalDivider(Modifier.padding(vertical = 4.dp))
                    DropdownMenuItem(
                        text = { Text("Subject (A–Z)") },
                        onClick = {
                            sortMenuExpanded = false
                            onSortChange("subject", false)
                        },
                    )
                }
            }
        }

        HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.4f))

        // --- Message Rows ---
        if (searching) {
            LazyColumn(Modifier.weight(1f)) {
                items(hits, key = { "${it.folder_id}:${it.uid}" }) { h ->
                    val isSelected = h.uid == currentUid
                    SearchHitRow(
                        hit = h,
                        isSelected = isSelected,
                        onSelect = { onOpenSearchHit(h) },
                        onToggleStar = { onToggleStar(h.uid, h.starred) },
                    )
                }
                if (hits.isEmpty()) {
                    item {
                        Text(
                            "No matching messages found.",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(16.dp),
                        )
                    }
                }
            }
        } else {
            LazyColumn(Modifier.weight(1f)) {
                items(rows, key = { it.uid }) { m ->
                    val isSelected = m.uid == currentUid
                    MessageItemRow(
                        message = m,
                        isSelected = isSelected,
                        onSelect = { onSelect(m.uid) },
                        onToggleStar = { onToggleStar(m.uid, m.starred) },
                    )
                }
                if (rows.isEmpty()) {
                    item {
                        Text(
                            if (currentFolderId == null) "Select a folder from the sidebar."
                            else "Empty folder.",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(16.dp),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun MessageItemRow(
    message: MessageRow,
    isSelected: Boolean,
    onSelect: () -> Unit,
    onToggleStar: () -> Unit,
) {
    Box(
        Modifier
            .fillMaxWidth()
            .clickable { onSelect() }
            .background(
                if (isSelected) MaterialTheme.colorScheme.primary.copy(alpha = 0.15f)
                else Color.Transparent,
            ),
    ) {
        // Selection indicator bar on the left (QML idiom)
        if (isSelected) {
            Box(
                Modifier
                    .width(3.dp)
                    .fillMaxHeight()
                    .background(MaterialTheme.colorScheme.primary),
            )
        }

        Row(
            Modifier
                .fillMaxWidth()
                .padding(start = 8.dp, end = 10.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            // Unread dot
            Box(
                Modifier.size(8.dp),
                contentAlignment = Alignment.Center,
            ) {
                if (message.unread) {
                    Box(
                        Modifier
                            .size(7.dp)
                            .clip(CircleShape)
                            .background(UnreadAccent),
                    )
                }
            }

            // Sender Avatar
            SenderAvatar(
                seed = message.from,
                size = 32.dp,
                fontSize = 12.sp,
            )

            // Content column
            Column(
                Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                // Top line: Sender name + attachment icon + Date
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = message.from.ifBlank { "(unknown)" },
                        fontWeight = if (message.unread) FontWeight.Bold else FontWeight.Medium,
                        fontSize = 14.sp,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    if (message.has_attachments) {
                        Text(
                            text = "📎 ",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    Text(
                        text = message.date,
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.End,
                    )
                }

                // Middle line: Subject
                Text(
                    text = message.subject.ifBlank { "(no subject)" },
                    fontWeight = if (message.unread) FontWeight.Bold else FontWeight.Normal,
                    fontSize = 14.sp,
                    color = if (message.unread) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.85f),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )

                // Bottom line: Snippet preview
                if (message.snippet.isNotBlank()) {
                    Text(
                        text = message.snippet,
                        fontSize = 13.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }

            // Star icon button
            Box(
                Modifier
                    .size(24.dp)
                    .clip(CircleShape)
                    .clickable { onToggleStar() },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = if (message.starred) "★" else "☆",
                    color = if (message.starred) StarOn else StarOff,
                    fontSize = 16.sp,
                )
            }
        }

        // Bottom divider
        HorizontalDivider(
            Modifier.align(Alignment.BottomCenter),
            color = MaterialTheme.colorScheme.outline.copy(alpha = 0.25f),
        )
    }
}

@Composable
private fun SearchHitRow(
    hit: SearchHit,
    isSelected: Boolean,
    onSelect: () -> Unit,
    onToggleStar: () -> Unit,
) {
    Box(
        Modifier
            .fillMaxWidth()
            .clickable { onSelect() }
            .background(
                if (isSelected) MaterialTheme.colorScheme.primary.copy(alpha = 0.15f)
                else Color.Transparent,
            ),
    ) {
        if (isSelected) {
            Box(
                Modifier
                    .width(3.dp)
                    .fillMaxHeight()
                    .background(MaterialTheme.colorScheme.primary),
            )
        }

        Row(
            Modifier
                .fillMaxWidth()
                .padding(start = 8.dp, end = 10.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Box(Modifier.size(8.dp), contentAlignment = Alignment.Center) {
                if (hit.unread) {
                    Box(
                        Modifier
                            .size(7.dp)
                            .clip(CircleShape)
                            .background(UnreadAccent),
                    )
                }
            }

            SenderAvatar(
                seed = hit.from,
                size = 32.dp,
                fontSize = 12.sp,
            )

            Column(
                Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Row(
                    Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = hit.from.ifBlank { "(unknown)" },
                        fontWeight = if (hit.unread) FontWeight.Bold else FontWeight.Medium,
                        fontSize = 13.sp,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    if (hit.has_attachments) {
                        Text("📎 ", fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    Text(
                        text = hit.date,
                        fontSize = 11.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }

                Text(
                    text = hit.subject.ifBlank { "(no subject)" },
                    fontWeight = if (hit.unread) FontWeight.Bold else FontWeight.Normal,
                    fontSize = 13.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )

                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        text = "[${hit.folder}]",
                        fontSize = 10.sp,
                        fontWeight = FontWeight.SemiBold,
                        color = MaterialTheme.colorScheme.primary,
                    )
                    if (hit.snippet.isNotBlank()) {
                        Text(
                            text = hit.snippet,
                            fontSize = 11.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f),
                        )
                    }
                }
            }

            Box(
                Modifier
                    .size(24.dp)
                    .clip(CircleShape)
                    .clickable { onToggleStar() },
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = if (hit.starred) "★" else "☆",
                    color = if (hit.starred) StarOn else StarOff,
                    fontSize = 15.sp,
                )
            }
        }

        HorizontalDivider(
            Modifier.align(Alignment.BottomCenter),
            color = MaterialTheme.colorScheme.outline.copy(alpha = 0.25f),
        )
    }
}
