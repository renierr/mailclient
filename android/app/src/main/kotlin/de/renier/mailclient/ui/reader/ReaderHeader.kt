package de.renier.mailclient.ui.reader

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.Avatar
import org.json.JSONObject

/** One non-inline attachment as the card lists it (feed fields). */
data class Attachment(
    val id: Long,
    val name: String,
    val fileName: String,
    val sizeText: String,
    val mime: String,
)

fun attachmentsOf(m: JSONObject): List<Attachment> {
    val eventId = m.optJSONObject("event")?.let { if (it.isNull("attachment_id")) null else it.optLong("attachment_id") }
    val arr = m.optJSONArray("attachments") ?: return emptyList()
    return (0 until arr.length()).map { arr.getJSONObject(it) }
        .filter { !it.optBoolean("is_inline") && it.optLong("id") != eventId }
        .map {
            Attachment(
                id = it.optLong("id"),
                name = it.optString("display_name").ifEmpty { it.optString("filename") },
                fileName = it.optString("file_name").ifEmpty { "attachment.bin" },
                sizeText = it.optString("size_text"),
                // The core's opener type (`mime::open_mime`), never guessed here.
                mime = it.optString("open_mime", "*/*").ifEmpty { "*/*" },
            )
        }
}

// The reader header (Flutter ReaderHeader): subject, sender with avatar and
// date, To line, reply-to warning, expandable details, then the cards —
// calendar invite, missing inline images, attachments. It scrolls away with
// the body; nothing here is pinned.
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ReaderHeader(
    m: JSONObject,
    headers: JSONObject,
    detailsExpanded: Boolean,
    onToggleDetails: () -> Unit,
    downloadingInline: Boolean,
    onDownloadInline: () -> Unit,
    onOpenAttachment: (Attachment) -> Unit,
    onSaveAttachment: (Attachment) -> Unit,
    onSaveAll: (List<Attachment>) -> Unit,
    onOpenEvent: (Long) -> Unit,
    onSaveEvent: (Long, String) -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    Column(
        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            m.optString("subject", "(no subject)"),
            style = MaterialTheme.typography.headlineSmall,
            modifier = Modifier.padding(end = 8.dp),
        )
        Row(verticalAlignment = Alignment.Top) {
            Avatar(
                initials = m.optString("initials", "?").ifEmpty { "?" },
                avatarLight = m.optString("avatar_light"),
                avatarDark = m.optString("avatar_dark"),
            )
            Column(modifier = Modifier.weight(1f).padding(start = 12.dp)) {
                val name = m.optString("from_name")
                val addr = m.optString("from")
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        name.ifEmpty { addr },
                        style = MaterialTheme.typography.titleSmall,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    Text(
                        "  " + m.optString("date").ifEmpty { headers.optString("date") },
                        style = MaterialTheme.typography.labelMedium,
                        color = scheme.onSurfaceVariant,
                        maxLines = 1,
                    )
                }
                if (name.isNotEmpty()) Muted(addr)
                val to = m.optString("to").ifEmpty { headers.optString("to") }
                if (to.isNotEmpty() && !detailsExpanded) Muted("to $to")
                val replyTo = m.optString("reply_to")
                if (m.optBoolean("reply_to_differs") && replyTo.isNotEmpty()) {
                    Text(
                        "Replies go to $replyTo, not to the sender",
                        style = MaterialTheme.typography.bodySmall,
                        color = scheme.error,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            IconButton(onClick = onToggleDetails) {
                Icon(
                    painter = painterResource(if (detailsExpanded) R.drawable.ic_expand_more else R.drawable.ic_chevron_right),
                    contentDescription = if (detailsExpanded) "Hide details" else "Show details",
                    tint = scheme.onSurfaceVariant,
                )
            }
        }
        if (detailsExpanded) Details(m, headers)

        m.optJSONObject("event")?.let { EventCard(it, onOpenEvent, onSaveEvent) }

        val missing = m.optInt("missing_inline_images")
        if (m.optBoolean("is_html") && missing > 0) {
            ReaderCard {
                FlowRow(verticalArrangement = Arrangement.Center, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        "$missing inline image${if (missing == 1) "" else "s"} not downloaded",
                        modifier = Modifier.align(Alignment.CenterVertically),
                    )
                    TextButton(onClick = onDownloadInline, enabled = !downloadingInline) {
                        Text(if (downloadingInline) "Downloading…" else "Download")
                    }
                }
            }
        }

        val files = attachmentsOf(m)
        if (files.isNotEmpty()) {
            ReaderCard {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(painterResource(R.drawable.ic_attach), null, modifier = Modifier.size(18.dp))
                    Text(
                        if (files.size == 1) "1 attachment" else "${files.size} attachments",
                        style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.weight(1f).padding(start = 8.dp),
                    )
                    if (files.size > 1) TextButton(onClick = { onSaveAll(files) }) { Text("Save all") }
                }
                for (a in files) {
                    Row(
                        modifier = Modifier.fillMaxWidth().clickable { onOpenAttachment(a) },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(modifier = Modifier.weight(1f).padding(vertical = 6.dp)) {
                            Text(a.name, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Muted(a.sizeText)
                        }
                        IconButton(onClick = { onSaveAttachment(a) }) {
                            Icon(painterResource(R.drawable.ic_download), "Save ${a.name}", tint = scheme.onSurfaceVariant)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Muted(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
}

@Composable
private fun Details(m: JSONObject, h: JSONObject) {
    val rows = listOf(
        "From" to h.optString("from").ifEmpty { m.optString("from") },
        "To" to h.optString("to"),
        "Cc" to h.optString("cc"),
        "Date" to h.optString("date"),
        "Reply-To" to h.optString("reply_to"),
    ).filter { it.second.isNotEmpty() }
    Column(modifier = Modifier.padding(start = 52.dp, end = 8.dp)) {
        for ((label, value) in rows) {
            Row(modifier = Modifier.padding(vertical = 2.dp)) {
                Text(
                    label,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.width(64.dp),
                )
                Text(value, style = MaterialTheme.typography.bodySmall, modifier = Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun ReaderCard(alert: Boolean = false, content: @Composable () -> Unit) {
    OutlinedCard(
        modifier = Modifier.fillMaxWidth().padding(end = 8.dp),
        border = if (alert) BorderStroke(1.dp, MaterialTheme.colorScheme.error) else androidx.compose.material3.CardDefaults.outlinedCardBorder(),
    ) {
        Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) { content() }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun EventCard(event: JSONObject, onOpen: (Long) -> Unit, onSave: (Long, String) -> Unit) {
    val cancelled = event.optBoolean("is_cancelled")
    ReaderCard(alert = cancelled) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(painterResource(R.drawable.ic_event), null, modifier = Modifier.size(18.dp))
            Text(
                (if (cancelled) "Cancelled: " else "") + event.optString("summary", "(Event)"),
                style = MaterialTheme.typography.titleSmall,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(start = 8.dp),
            )
        }
        Muted(event.optString("formatted_time"))
        event.optString("location").takeIf { it.isNotBlank() }?.let { Muted("Where: $it") }
        event.optString("organizer").takeIf { it.isNotBlank() }?.let { Muted("Organizer: $it") }
        if (!event.isNull("attachment_id")) {
            val id = event.optLong("attachment_id")
            val name = event.optString("save_name", "event.ics").ifEmpty { "event.ics" }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.End), modifier = Modifier.fillMaxWidth()) {
                TextButton(onClick = { onOpen(id) }) { Text("Open in Calendar") }
                TextButton(onClick = { onSave(id, name) }) { Text("Save .ics") }
            }
        }
    }
}
