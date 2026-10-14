package de.renier.mailclient.ui.outbox

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.MailSendAlarm
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.DeleteConfirmDialog
import de.renier.mailclient.ui.common.strings
import de.renier.mailclient.ui.composer.ScheduleDialog
import de.renier.mailclient.ui.composer.nextTryLabel
import de.renier.mailclient.ui.composer.sendsAtLabel
import de.renier.mailclient.ui.settings.BackgroundPower
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.refreshOutbox
import de.renier.mailclient.ui.state.syncNow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/** One unsent mail, as `mailcore::outbox::list_json` describes it. */
private data class OutboxEntry(
    val id: Long,
    // The core's row model: sending / scheduled / queued / retrying / failed.
    val kind: String,
    val subject: String,
    val to: String,
    // The core's one-line state ("Queued — goes out with the next sync").
    val state: String,
    val error: String,
    // UTC instants: a scheduled row's time, a retrying row's next try.
    val sendAt: String,
    val retryAt: String,
    // Delivery goes out to the own address when this one sends.
    val notifySelf: Boolean,
    // What the core offers for this row (`outbox::RowActions`).
    val canSendNow: Boolean,
    val sendNowLabel: String,
    val canReschedule: Boolean,
    val canEdit: Boolean,
    val canDelete: Boolean,
)

private fun parse(json: String): List<OutboxEntry> {
    val arr = JSONArray(json)
    return (0 until arr.length()).mapNotNull { i ->
        val o = arr.optJSONObject(i) ?: return@mapNotNull null
        val actions = o.optJSONObject("actions") ?: JSONObject()
        OutboxEntry(
            id = o.optLong("id"),
            kind = o.optString("kind"),
            subject = o.optString("subject"),
            to = o.optJSONArray("envelope_to").strings().joinToString(", "),
            state = o.optString("state"),
            error = o.optString("last_error"),
            sendAt = o.optString("send_at"),
            retryAt = o.optString("retry_at"),
            notifySelf = o.optBoolean("notify_self"),
            canSendNow = actions.optBoolean("can_send_now"),
            sendNowLabel = actions.optString("send_now_label", "Send now"),
            canReschedule = actions.optBoolean("can_reschedule"),
            canEdit = actions.optBoolean("can_edit"),
            canDelete = actions.optBoolean("can_delete"),
        )
    }
}

/**
 * Unsent mail of the open account: scheduled, queued, retrying or failed
 * sends, as the core describes them (`kind`, `state`, `actions`). Failed
 * sends retry by themselves for a while, then stop and wait here: Retry,
 * Reschedule, Edit (back into the composer, through [onEdit]) or Delete
 * after a confirm. A page on a phone, the list pane beside the folders on
 * wider layouts. Re-reads whenever the outbox counts move or a sync ends.
 */
@Composable
fun OutboxScreen(state: MailState, onEdit: (Long) -> Unit) {
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val accountId = state.activeAccountId
    var entries by remember { mutableStateOf<List<OutboxEntry>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var forget by remember { mutableStateOf<OutboxEntry?>(null) }
    var reschedule by remember { mutableStateOf<OutboxEntry?>(null) }

    LaunchedEffect(accountId, reload, state.outboxPending, state.outboxFailed, state.outboxRetryable, state.syncing) {
        if (accountId < 0) {
            entries = emptyList()
            return@LaunchedEffect
        }
        withContext(Dispatchers.IO) { runCatching { parse(MailNative.outboxJson(accountId)) } }
            .onSuccess { entries = it }
            .onFailure { error = it.message ?: "Could not read the outbox" }
    }

    // The held rows changed: the exact alarm plans from them again.
    fun refreshAlarm() {
        scope.launch {
            withContext(Dispatchers.IO) { runCatching { MailSendAlarm.refresh(context) } }
        }
    }

    fun dismiss(e: OutboxEntry) {
        scope.launch {
            withContext(Dispatchers.IO) { runCatching { MailNative.dismissOutbox(accountId, e.id) } }
                .onFailure { error = it.message ?: "Could not delete the entry" }
            state.refreshOutbox()
            refreshAlarm()
            reload++
        }
    }

    fun sendNow(e: OutboxEntry) {
        scope.launch {
            withContext(Dispatchers.IO) { runCatching { MailNative.sendNowOutbox(accountId, e.id) } }
                .onSuccess { state.info("Sending now") }
                .onFailure { error = it.message ?: "Could not queue for sending" }
            state.refreshOutbox()
            refreshAlarm()
            reload++
        }
    }

    fun rescheduled(sendAtUtc: String) {
        val e = reschedule ?: return
        reschedule = null
        scope.launch {
            withContext(Dispatchers.IO) {
                runCatching { MailNative.rescheduleOutbox(accountId, e.id, sendAtUtc) }
            }
                .onSuccess { state.info("Rescheduled") }
                .onFailure { error = it.message ?: "Could not reschedule" }
            state.refreshOutbox()
            refreshAlarm()
            reload++
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
            Text(
                "Mail waiting to be sent from this phone. Failed sends try again by themselves for a while, then stop and wait for you.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 4.dp)) }
            if (entries?.any { it.kind == "scheduled" } == true && !BackgroundPower.exactAlarms(context)) {
                Text(
                    "Exact alarms are off: scheduled mail still sends, but may go out late while the phone sleeps.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(top = 4.dp),
                )
                OutlinedButton(
                    onClick = { BackgroundPower.requestExactAlarms(context) },
                    modifier = Modifier.padding(top = 4.dp),
                ) {
                    Text("Allow exact alarms")
                }
            }
            OutlinedButton(
                onClick = {
                    error = null
                    state.syncNow()
                },
                enabled = !state.syncing && state.outboxRetryable > 0,
                modifier = Modifier.padding(top = 8.dp),
            ) {
                if (state.syncing) {
                    CircularProgressIndicator(strokeWidth = 2.dp, modifier = Modifier.size(16.dp))
                } else {
                    Icon(painterResource(R.drawable.ic_sync), null, Modifier.size(16.dp))
                }
                Text("Sync now", modifier = Modifier.padding(start = 8.dp))
            }
        }
        HorizontalDivider()
        val list = entries
        when {
            list == null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
            list.isEmpty() -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                Text("Outbox is empty", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            else -> LazyColumn(modifier = Modifier.fillMaxSize()) {
                items(list, key = { it.id }) { e ->
                    OutboxRow(
                        e,
                        onDelete = { forget = e },
                        onSendNow = { sendNow(e) },
                        onReschedule = { reschedule = e },
                        onEdit = { onEdit(e.id) },
                    )
                    HorizontalDivider()
                }
            }
        }
    }

    forget?.let { e ->
        DeleteConfirmDialog(
            title = "Delete unsent mail?",
            text = "“${e.subject}” will not be sent and cannot be recovered.",
            confirmLabel = "Delete",
            onConfirm = {
                forget = null
                dismiss(e)
            },
            onDismiss = { forget = null },
        )
    }

    reschedule?.let { e ->
        ScheduleDialog(
            initialSendAtUtc = e.sendAt,
            initialNotifySelf = false,
            onConfirm = { sendAt, _ -> rescheduled(sendAt) },
            onDismiss = { reschedule = null },
            // Rescheduling keeps whatever the row already asks for: its own
            // `notify_self` survives the move, so the toggle is not offered.
            showReceipt = false,
        )
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun OutboxRow(
    e: OutboxEntry,
    onDelete: () -> Unit,
    onSendNow: () -> Unit,
    onReschedule: () -> Unit,
    onEdit: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val failed = e.kind == "failed"
    Row(
        verticalAlignment = Alignment.Top,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier
            .fillMaxWidth()
            .background(if (failed) scheme.errorContainer.copy(alpha = 0.35f) else scheme.surface)
            .padding(horizontal = 16.dp, vertical = 8.dp),
    ) {
        Icon(
            painterResource(
                when (e.kind) {
                    "scheduled" -> R.drawable.ic_schedule
                    "failed" -> R.drawable.ic_error
                    else -> R.drawable.ic_send
                },
            ),
            contentDescription = null,
            tint = if (failed) scheme.error else scheme.onSurfaceVariant,
            modifier = Modifier.padding(top = 2.dp),
        )
        Column(modifier = Modifier.weight(1f)) {
            Text(
                e.subject,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.Bold,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            if (e.to.isNotEmpty()) {
                Text(
                    e.to,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (e.sendAt.isNotEmpty()) {
                Text(sendsAtLabel(e.sendAt), style = MaterialTheme.typography.bodySmall, color = scheme.primary)
            }
            if (e.retryAt.isNotEmpty()) {
                Text(nextTryLabel(e.retryAt), style = MaterialTheme.typography.bodySmall, color = scheme.onSurfaceVariant)
            }
            if (e.notifySelf) {
                Text(
                    "Mails a delivery receipt to me when it sends",
                    style = MaterialTheme.typography.bodySmall,
                    color = scheme.primary,
                )
            }
            if (e.state.isNotEmpty()) {
                Text(
                    e.state,
                    style = MaterialTheme.typography.bodySmall,
                    color = if (failed) scheme.error else scheme.onSurfaceVariant,
                )
            }
            if (e.error.isNotEmpty()) {
                Text(e.error, style = MaterialTheme.typography.bodySmall, color = scheme.error)
            }
            // Never a row being sent: the core offers nothing for it. Wraps
            // under each other at 360dp and large text.
            if (e.canSendNow || e.canReschedule || e.canEdit || e.canDelete) {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                    modifier = Modifier.padding(top = 4.dp),
                ) {
                    if (e.canSendNow) OutlinedButton(onClick = onSendNow) { Text(e.sendNowLabel) }
                    if (e.canReschedule) OutlinedButton(onClick = onReschedule) { Text("Reschedule") }
                    if (e.canEdit) OutlinedButton(onClick = onEdit) { Text("Edit") }
                    if (e.canDelete) {
                        OutlinedButton(onClick = onDelete) { Text("Delete", color = scheme.error) }
                    }
                }
            }
        }
    }
}
