package de.renier.mailclient.ui.reader

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
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
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
    val arr = m.optJSONArray("attachments") ?: return emptyList()
    // `in_card`: an event or contact card above already opens and saves it.
    return (0 until arr.length()).map { arr.getJSONObject(it) }
        .filter { !it.optBoolean("is_inline") && !it.optBoolean("in_card") }
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

// The reader header (Flutter ReaderHeader): back in front of the subject
// where the layout shows no back of its own, sender with avatar and date,
// To line, reply-to warning, expandable details, then the message actions
// right-aligned on their own row — and then the cards (ReaderCards.kt:
// calendar invite, delivery report, contact cards, attached mails; then
// missing inline images and attachments). It all scrolls away with the
// body; nothing here is pinned.
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
    onOpenContact: (Long) -> Unit,
    onSaveContact: (Long, String) -> Unit,
    onOpenAttachedMail: (Long) -> Unit,
    onSaveAttachedMail: (Long, String) -> Unit,
    // "Edit & resend" on a bounce: the sent original's folder and uid.
    onResend: (Long, Int) -> Unit,
    // "Open sent mail" on a report or receipt: the original's folder and uid.
    onOpenOriginal: (Long, Int) -> Unit,
    // Back in front of the subject, like Flutter: the one-pane reader and
    // the two-pane reader show the arrow, three panes the close icon.
    // Null hides it.
    onClose: (() -> Unit)? = null,
    closeIcon: Boolean = false,
    // Archive, delete, star, colours, fullscreen and the ⋮ menu: part of
    // the scrolling header, never pinned above it. Null hides the row.
    actions: (@Composable () -> Unit)? = null,
) {
    val scheme = MaterialTheme.colorScheme
    Column(
        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (onClose != null) {
                IconButton(onClick = onClose) {
                    if (closeIcon) {
                        Icon(painterResource(R.drawable.ic_close), "Close")
                    } else {
                        Icon(painterResource(R.drawable.ic_arrow_back), "Back")
                    }
                }
            }
            Text(
                m.optString("subject", "(no subject)"),
                style = MaterialTheme.typography.headlineSmall,
                modifier = Modifier.weight(1f).padding(end = 8.dp),
            )
        }
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

        if (actions != null) {
            // Right-aligned like the Qt action row; FlowRow so a narrow
            // pane stacks them instead of overflowing.
            FlowRow(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.End,
                verticalArrangement = Arrangement.Center,
            ) { actions() }
        }

        m.optJSONObject("event")?.let { EventCard(it, onOpenEvent, onSaveEvent) }

        m.optJSONObject("report")?.let { ReportCard(it, downloadingInline, onDownloadInline, onResend, onOpenOriginal) }

        m.optJSONArray("contacts")?.let { cards ->
            for (i in 0 until cards.length()) {
                ContactCard(cards.getJSONObject(i), downloadingInline, onDownloadInline, onOpenContact, onSaveContact)
            }
        }

        m.optJSONArray("attached_messages")?.let { mails ->
            for (i in 0 until mails.length()) {
                AttachedMessageCard(
                    mails.getJSONObject(i),
                    downloadingInline,
                    onDownloadInline,
                    onOpenAttachedMail,
                    onSaveAttachedMail,
                )
            }
        }

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

        val files = remember(m) { attachmentsOf(m) }
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
