package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import mailclient.models.AttachmentMeta
import mailclient.models.MessageDetail

/**
 * Right pane: reader with header block, compact action buttons,
 * collapsible details, attachment list, and styled message body (QML MessageView.qml).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun MessageViewPane(
    detail: MessageDetail?,
    onReply: (MessageDetail) -> Unit = {},
    onReplyAll: (MessageDetail) -> Unit = {},
    onForward: (MessageDetail) -> Unit = {},
    onToggleStar: (Long, Boolean) -> Unit = { _, _ -> },
    onDelete: (Long) -> Unit = {},
    onArchive: (Long) -> Unit = {},
    onOpenAttachment: (Long) -> Unit = {},
    modifier: Modifier = Modifier,
) {
    var showHeadersDialog by remember { mutableStateOf(false) }
    var headerExpanded by remember { mutableStateOf(false) }
    var moreMenuExpanded by remember { mutableStateOf(false) }

    Column(
        modifier
            .background(MaterialTheme.colorScheme.background)
            .fillMaxSize(),
    ) {
        if (detail == null) {
            Column(
                Modifier.fillMaxSize(),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    "✉",
                    fontSize = 38.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f),
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    "Select a message to read",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            return@Column
        }

        // --- Header Section ---
        Column(
            Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.25f))
                .padding(14.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            // Subject
            Text(
                text = detail.subject.ifBlank { "(no subject)" },
                fontWeight = FontWeight.Bold,
                fontSize = 20.sp,
                color = MaterialTheme.colorScheme.onSurface,
            )

            // Sender Row
            Row(
                Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                SenderAvatar(
                    seed = detail.from,
                    size = 38.dp,
                    fontSize = 15.sp,
                )

                Column(Modifier.weight(1f)) {
                    Text(
                        text = detail.from,
                        fontWeight = FontWeight.Bold,
                        fontSize = 14.5.sp,
                        color = MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = detail.date,
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (detail.reply_to.isNotBlank() && !detail.reply_to.equals(detail.from, ignoreCase = true)) {
                        Text(
                            text = "↩ Replies go to ${detail.reply_to}, not to sender",
                            fontSize = 12.sp,
                            color = MaterialTheme.colorScheme.error,
                        )
                    }
                }

                // Details Toggle
                Row(
                    Modifier
                        .clip(RoundedCornerShape(4.dp))
                        .clickable { headerExpanded = !headerExpanded }
                        .padding(horizontal = 6.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text(
                        text = if (headerExpanded) "Details ⌃" else "Details ⌄",
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }

            // Expanded Details Grid
            if (headerExpanded) {
                Column(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(6.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f))
                        .padding(8.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    HeaderDetailRow("From", detail.from)
                    HeaderDetailRow("Date", detail.date)
                    HeaderDetailRow("Subject", detail.subject)
                    if (detail.reply_to.isNotBlank()) HeaderDetailRow("Reply-To", detail.reply_to)
                }
            }

            // Action Buttons (FlowRow to prevent horizontal overflow)
            FlowRow(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                DesktopActionButton("↩ Reply") { onReply(detail) }
                DesktopActionButton("↩↩ All") { onReplyAll(detail) }
                DesktopActionButton("→ Forward") { onForward(detail) }
                DesktopActionButton(
                    label = if (detail.starred) "★ Starred" else "☆ Star",
                    textColor = if (detail.starred) StarOn else null,
                ) {
                    onToggleStar(detail.uid, detail.starred)
                }
                DesktopActionButton("🗄 Archive") { onArchive(detail.uid) }
                DesktopActionButton(
                    label = "🗑 Delete",
                    textColor = MaterialTheme.colorScheme.error,
                ) {
                    onDelete(detail.uid)
                }

                Box {
                    DesktopActionButton("⋯ More") { moreMenuExpanded = true }
                    DropdownMenu(
                        expanded = moreMenuExpanded,
                        onDismissRequest = { moreMenuExpanded = false },
                    ) {
                        DropdownMenuItem(
                            text = { Text("Show raw headers…") },
                            onClick = {
                                moreMenuExpanded = false
                                showHeadersDialog = true
                            },
                        )
                    }
                }
            }
        }

        HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.4f))

        // --- Attachments Section ---
        if (detail.attachments.isNotEmpty()) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 14.dp, vertical = 8.dp),
            ) {
                Text(
                    text = "Attachments (${detail.attachments.size})",
                    fontWeight = FontWeight.Bold,
                    fontSize = 12.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(4.dp))
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    detail.attachments.forEach { a ->
                        AttachmentChip(a, onOpen = { onOpenAttachment(a.id) })
                    }
                }
            }
            HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.3f))
        }

        // --- Message Body ---
        SelectionContainer(
            Modifier
                .fillMaxWidth()
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .padding(16.dp),
        ) {
            Text(
                text = detail.readableText(),
                fontSize = 15.sp,
                lineHeight = 24.sp,
                color = MaterialTheme.colorScheme.onSurface,
            )
        }
    }

    // Raw Headers Dialog
    if (showHeadersDialog && detail != null) {
        Dialog(onDismissRequest = { showHeadersDialog = false }) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(10.dp))
                    .background(MaterialTheme.colorScheme.surface)
                    .border(1.dp, MaterialTheme.colorScheme.outline, RoundedCornerShape(10.dp))
                    .padding(18.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Text("Message Headers", fontWeight = FontWeight.Bold, fontSize = 16.sp)
                Column(
                    Modifier
                        .fillMaxWidth()
                        .verticalScroll(rememberScrollState())
                        .weight(1f, fill = false),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    HeaderDetailRow("From", detail.from)
                    HeaderDetailRow("Date", detail.date)
                    HeaderDetailRow("Subject", detail.subject)
                    if (detail.reply_to.isNotBlank()) HeaderDetailRow("Reply-To", detail.reply_to)
                }
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.End,
                ) {
                    TextButton(onClick = { showHeadersDialog = false }) {
                        Text("Close")
                    }
                }
            }
        }
    }
}

@Composable
private fun DesktopActionButton(
    label: String,
    textColor: Color? = null,
    onClick: () -> Unit,
) {
    Box(
        Modifier
            .clip(RoundedCornerShape(5.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.6f))
            .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f), RoundedCornerShape(5.dp))
            .clickable { onClick() }
            .padding(horizontal = 10.dp, vertical = 5.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = label,
            fontSize = 13.sp,
            fontWeight = FontWeight.Medium,
            color = textColor ?: MaterialTheme.colorScheme.onSurface,
        )
    }
}

@Composable
private fun AttachmentChip(
    attachment: AttachmentMeta,
    onOpen: () -> Unit,
) {
    Row(
        Modifier
            .clip(RoundedCornerShape(6.dp))
            .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.7f))
            .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f), RoundedCornerShape(6.dp))
            .clickable { onOpen() }
            .padding(horizontal = 8.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Text("📎", fontSize = 12.sp)
        Text(
            text = attachment.filename ?: "attachment-${attachment.id}.bin",
            fontSize = 12.sp,
            fontWeight = FontWeight.Medium,
            color = MaterialTheme.colorScheme.primary,
        )
        Text(
            text = formatSize(attachment.size),
            fontSize = 11.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun HeaderDetailRow(label: String, value: String) {
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = "$label:",
            fontSize = 11.sp,
            fontWeight = FontWeight.Bold,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.width(60.dp),
        )
        Text(
            text = value.ifBlank { "—" },
            fontSize = 12.sp,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.weight(1f),
        )
    }
}

private fun formatSize(bytes: Long): String = when {
    bytes < 1024 -> "$bytes B"
    bytes < 1024 * 1024 -> "${bytes / 1024} KB"
    else -> String.format("%.1f MB", bytes.toDouble() / (1024 * 1024))
}
