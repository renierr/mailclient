package de.renier.mailclient.ui.reader

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.Avatar
import de.renier.mailclient.ui.theme.toneColor
import org.json.JSONObject

// The preview cards between the reader header and the body: calendar
// invite, delivery report, contact cards and attached mails. Every field
// comes from the core's reader payload (mailcore::calendar / report /
// vcard / attached); this file only lays them out.

@Composable
internal fun Muted(text: String, maxLines: Int = 1) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        maxLines = maxLines,
        overflow = TextOverflow.Ellipsis,
    )
}

@Composable
internal fun ReaderCard(alert: Boolean = false, content: @Composable () -> Unit) {
    OutlinedCard(
        modifier = Modifier.fillMaxWidth().padding(end = 8.dp),
        border = if (alert) BorderStroke(1.dp, MaterialTheme.colorScheme.error) else CardDefaults.outlinedCardBorder(),
    ) {
        Column(modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp)) { content() }
    }
}

// A string field, null when absent, JSON null or blank (optString turns
// null into "null").
private fun optText(o: JSONObject, key: String): String? =
    if (o.isNull(key)) null else o.optString(key).takeIf { it.isNotBlank() }

private fun JSONObject.attachmentId(): Long? = if (isNull("attachment_id")) null else optLong("attachment_id")

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun CardActions(content: @Composable () -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.End), modifier = Modifier.fillMaxWidth()) {
        content()
    }
}

@Composable
private fun DownloadButton(downloading: Boolean, onDownload: () -> Unit) {
    TextButton(onClick = onDownload, enabled = !downloading) {
        Text(if (downloading) "Downloading…" else "Download")
    }
}

@Composable
internal fun EventCard(event: JSONObject, onOpen: (Long) -> Unit, onSave: (Long, String) -> Unit) {
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
        // Reply / counter / update: what this file says (core text + tone).
        optText(event, "notice")?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.SemiBold,
                color = toneColor(optText(event, "notice_tone") ?: "neutral"),
            )
        }
        Muted(event.optString("formatted_time"))
        optText(event, "location")?.let { Muted("Where: $it") }
        optText(event, "organizer")?.let { Muted("Organizer: $it") }
        event.attachmentId()?.let { id ->
            val name = event.optString("save_name", "event.ics").ifEmpty { "event.ics" }
            CardActions {
                TextButton(onClick = { onOpen(id) }) { Text("Open in Calendar") }
                TextButton(onClick = { onSave(id, name) }) { Text("Save .ics") }
            }
        }
    }
}

// A delivery report or read receipt (mailcore::report via the feed's
// `report`): outcome, what it means, each recipient with the reason in
// plain words and the server's own text, "Open sent mail" when the
// original is cached and "Edit & resend" for a failed delivery.
@Composable
internal fun ReportCard(
    report: JSONObject,
    downloading: Boolean,
    onDownload: () -> Unit,
    onResend: (folderId: Long, uid: Int) -> Unit,
    onOpenOriginal: (folderId: Long, uid: Int) -> Unit,
) {
    val loaded = report.optBoolean("loaded")
    val toneName = if (loaded) report.optString("tone", "neutral") else "neutral"
    val tone = toneColor(toneName)
    ReaderCard(alert = toneName == "negative") {
        SelectionContainer {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    val icon = when {
                        report.optString("kind") == "read" -> R.drawable.ic_mark_email_read
                        !loaded || toneName == "negative" -> R.drawable.ic_error
                        toneName == "warning" -> R.drawable.ic_schedule
                        toneName == "positive" -> R.drawable.ic_check
                        else -> R.drawable.ic_info
                    }
                    Icon(painterResource(icon), null, tint = tone, modifier = Modifier.size(20.dp))
                    Text(
                        report.optString("title"),
                        style = MaterialTheme.typography.titleSmall,
                        color = tone,
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
                Muted(report.optString("detail"), maxLines = 3)
                optText(report, "original_subject")?.let {
                    Text(
                        "Original: $it",
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                val recipients = report.optJSONArray("recipients")
                for (i in 0 until (recipients?.length() ?: 0)) {
                    val r = recipients!!.getJSONObject(i)
                    Column(modifier = Modifier.padding(start = 4.dp, top = 4.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                r.optString("address"),
                                style = MaterialTheme.typography.bodyMedium,
                                fontWeight = FontWeight.SemiBold,
                                maxLines = 2,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f, fill = false),
                            )
                            Text(
                                r.optString("action_label"),
                                style = MaterialTheme.typography.labelSmall,
                                color = toneColor(r.optString("tone")),
                                modifier = Modifier.padding(start = 8.dp),
                            )
                        }
                        optText(r, "reason")?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
                        optText(r, "diagnostic")?.let { Muted(it, maxLines = 4) }
                    }
                }
            }
        }
        val hasOriginal = !report.isNull("original_uid") && !report.isNull("original_folder_id")
        val canResend = report.optBoolean("can_resend") && hasOriginal
        if (canResend || hasOriginal || !loaded) {
            CardActions {
                if (!loaded) DownloadButton(downloading, onDownload)
                if (hasOriginal) {
                    TextButton(onClick = {
                        onOpenOriginal(report.optLong("original_folder_id"), report.optInt("original_uid"))
                    }) { Text("Open sent mail") }
                }
                if (canResend) {
                    TextButton(onClick = {
                        onResend(report.optLong("original_folder_id"), report.optInt("original_uid"))
                    }) { Text("Edit & resend") }
                }
            }
        }
    }
}

// A `.vcf` attachment as a card (mailcore::vcard via the feed's `contacts`):
// name with its badge, title/organisation, addresses, numbers and the
// postal address, selectable for copying. A card whose file is not cached
// yet offers the download that fills it in.
@Composable
internal fun ContactCard(
    card: JSONObject,
    downloading: Boolean,
    onDownload: () -> Unit,
    onOpen: (Long) -> Unit,
    onSave: (Long, String) -> Unit,
) {
    val loaded = card.optBoolean("loaded")
    ReaderCard {
        SelectionContainer {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (loaded) {
                        Avatar(
                            initials = card.optString("initials", "?").ifEmpty { "?" },
                            avatarLight = card.optString("avatar_light"),
                            avatarDark = card.optString("avatar_dark"),
                            size = 36.dp,
                        )
                    } else {
                        Icon(painterResource(R.drawable.ic_contacts), null, modifier = Modifier.size(18.dp))
                    }
                    Column(modifier = Modifier.weight(1f).padding(start = 12.dp)) {
                        Text(
                            card.optString("name", "(Contact)"),
                            style = MaterialTheme.typography.titleSmall,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                        optText(card, "affiliation")?.let { Muted(it) }
                        if (!loaded) Muted("Contact card not downloaded yet")
                    }
                }
                fieldsOf(card, "emails").forEach { (value, label) -> ContactLine(R.drawable.ic_mail, value, label) }
                fieldsOf(card, "phones").forEach { (value, label) -> ContactLine(R.drawable.ic_phone, value, label) }
                optText(card, "address")?.let { ContactLine(R.drawable.ic_place, it, null) }
                optText(card, "url")?.let { ContactLine(R.drawable.ic_link, it, null) }
                val more = card.optInt("more_cards")
                if (more > 0) Muted("+$more more contact${if (more == 1) "" else "s"} in this file")
            }
        }
        card.attachmentId()?.let { id ->
            val name = card.optString("save_name", "contact.vcf").ifEmpty { "contact.vcf" }
            CardActions {
                if (!loaded) DownloadButton(downloading, onDownload)
                TextButton(onClick = { onOpen(id) }) { Text("Open in Contacts") }
                TextButton(onClick = { onSave(id, name) }) { Text("Save .vcf") }
            }
        }
    }
}

private fun fieldsOf(card: JSONObject, key: String): List<Pair<String, String?>> {
    val arr = card.optJSONArray(key) ?: return emptyList()
    return (0 until arr.length()).map { arr.getJSONObject(it) }
        .mapNotNull { f -> optText(f, "value")?.let { it to optText(f, "label") } }
}

@Composable
private fun ContactLine(icon: Int, value: String, label: String?) {
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(start = 2.dp)) {
        Icon(
            painterResource(icon),
            null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(16.dp),
        )
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f, fill = false).padding(start = 8.dp),
        )
        if (label != null) {
            Text(
                label,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                modifier = Modifier.padding(start = 8.dp),
            )
        }
    }
}

// A mail attached to the open one (mailcore::attached via the feed's
// `attached_messages`): subject, sender and date, snippet, and its plain
// text body to expand — no remote content, so expanding is always safe.
@Composable
internal fun AttachedMessageCard(
    mail: JSONObject,
    downloading: Boolean,
    onDownload: () -> Unit,
    onOpen: (Long) -> Unit,
    onSave: (Long, String) -> Unit,
) {
    val loaded = mail.optBoolean("loaded")
    var expanded by rememberSaveable(mail.optLong("attachment_id")) { mutableStateOf(false) }
    val body = optText(mail, "body_text")
    ReaderCard {
        SelectionContainer {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(painterResource(R.drawable.ic_mail), null, modifier = Modifier.size(18.dp))
                    Text(
                        mail.optString("subject", "(no subject)"),
                        style = MaterialTheme.typography.titleSmall,
                        maxLines = 3,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
                optText(mail, "byline")?.let { Muted(it, maxLines = 2) }
                optText(mail, "to")?.let { Muted("To: $it") }
                if (!loaded) Muted("Attached message not downloaded yet")
                if (expanded && body != null) {
                    Text(body, style = MaterialTheme.typography.bodySmall)
                } else {
                    optText(mail, "snippet")?.let {
                        Text(it, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    }
                }
                val n = mail.optInt("attachment_count")
                if (loaded && n > 0) Muted(if (n == 1) "1 attachment inside" else "$n attachments inside")
            }
        }
        mail.attachmentId()?.let { id ->
            val name = mail.optString("save_name", "message.eml").ifEmpty { "message.eml" }
            CardActions {
                if (!loaded) DownloadButton(downloading, onDownload)
                if (loaded && body != null) {
                    TextButton(onClick = { expanded = !expanded }) {
                        Text(if (expanded) "Hide message" else "Show message")
                    }
                }
                TextButton(onClick = { onOpen(id) }) { Text("Open") }
                TextButton(onClick = { onSave(id, name) }) { Text("Save .eml") }
            }
        }
    }
}
