package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import mailclient.models.MessageDetail

/**
 * Right pane: reader with header block, action bar, plain body and attachment bar
 * (QML MessageView.qml).
 */
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
    var showHeaders by remember { mutableStateOf(false) }
    Column(modifier.padding(14.dp)) {
        val d = detail
        if (d == null) {
            Column(
                Modifier.fillMaxWidth().weight(1f),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text(
                    "Select a message to read.",
                    fontSize = 15.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            return@Column
        }

        // Header section
        Text(d.subject, fontWeight = FontWeight.Bold, fontSize = 20.sp)
        Text(
            "${d.from} · ${d.date}",
            fontSize = 13.sp,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 4.dp),
        )
        if (d.reply_to.isNotBlank()) {
            Text(
                "Reply-To: ${d.reply_to}",
                fontSize = 12.sp,
                color = UnreadAccent,
                modifier = Modifier.padding(top = 2.dp),
            )
        }
        if (d.has_remote_images) {
            Text(
                "Remote images blocked.",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 2.dp),
            )
        }

        // Action buttons
        Row(
            Modifier.fillMaxWidth().padding(top = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Button(onClick = { onReply(d) }) {
                Text("↩ Reply")
            }
            OutlinedButton(onClick = { onReplyAll(d) }) {
                Text("↩↩ All")
            }
            OutlinedButton(onClick = { onForward(d) }) {
                Text("↪ Forward")
            }
            OutlinedButton(onClick = { onToggleStar(d.uid, d.starred) }) {
                Text(
                    if (d.starred) "★ Starred" else "☆ Star",
                    color = if (d.starred) StarOn else MaterialTheme.colorScheme.onSurface,
                )
            }
            OutlinedButton(onClick = { onArchive(d.uid) }) {
                Text("📁 Archive")
            }
            OutlinedButton(
                onClick = { onDelete(d.uid) },
                colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
            ) {
                Text("🗑 Delete")
            }
            Spacer(Modifier.weight(1f))
            TextButton(onClick = { showHeaders = true }) {
                Text("Headers")
            }
        }

        HorizontalDivider(Modifier.fillMaxWidth().padding(vertical = 10.dp))

        // Attachments
        if (d.attachments.isNotEmpty()) {
            Text("Attachments (${d.attachments.size}):", fontWeight = FontWeight.Bold, fontSize = 13.sp)
            Row(
                Modifier.fillMaxWidth().padding(vertical = 6.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                d.attachments.forEach { a ->
                    OutlinedButton(
                        onClick = { onOpenAttachment(a.id) },
                    ) {
                        Text("📎 ${a.filename ?: "unnamed"} (${formatSize(a.size)})", fontSize = 12.sp)
                    }
                }
            }
            HorizontalDivider(Modifier.fillMaxWidth().padding(vertical = 10.dp))
        }

        // Message Body
        Column(Modifier.verticalScroll(rememberScrollState()).weight(1f)) {
            Text(d.readableText(), fontSize = 14.sp, lineHeight = 22.sp)
        }

        if (showHeaders) {
            Dialog(onDismissRequest = { showHeaders = false }) {
                Column(
                    Modifier
                        .fillMaxWidth()
                        .background(MaterialTheme.colorScheme.surface, RoundedCornerShape(12.dp))
                        .padding(20.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text("Message Headers", fontWeight = FontWeight.Bold, fontSize = 16.sp)
                    HeaderLine("From", d.from)
                    HeaderLine("Date", d.date)
                    HeaderLine("Subject", d.subject)
                    if (d.reply_to.isNotBlank()) HeaderLine("Reply-To", d.reply_to)
                    Row(horizontalArrangement = Arrangement.End, modifier = Modifier.fillMaxWidth()) {
                        TextButton(onClick = { showHeaders = false }) { Text("Close") }
                    }
                }
            }
        }
    }
}

@Composable
private fun HeaderLine(name: String, value: String) {
    Column {
        Text(name, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value.ifBlank { "—" }, fontSize = 13.sp)
    }
}

private fun formatSize(bytes: Long): String = when {
    bytes < 1024 -> "$bytes B"
    bytes < 1024 * 1024 -> "${bytes / 1024} KiB"
    else -> "${bytes / (1024 * 1024)} MiB"
}
