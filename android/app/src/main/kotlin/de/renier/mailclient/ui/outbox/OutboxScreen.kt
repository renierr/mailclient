package de.renier.mailclient.ui.outbox

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.R
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.refreshOutbox
import de.renier.mailclient.ui.state.syncNow
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray

/** One unsent mail, as `mailcore::outbox::list_json` describes it. */
private data class OutboxEntry(
    val id: Long,
    val subject: String,
    val to: String,
    // The core's one-line state ("Queued — goes out with the next sync").
    val state: String,
    val error: String,
    val dismissable: Boolean,
)

private fun parse(json: String): List<OutboxEntry> {
    val arr = JSONArray(json)
    return (0 until arr.length()).mapNotNull { i ->
        val o = arr.optJSONObject(i) ?: return@mapNotNull null
        val to = o.optJSONArray("envelope_to")
        OutboxEntry(
            id = o.optLong("id"),
            subject = o.optString("subject"),
            to = (0 until (to?.length() ?: 0)).joinToString(", ") { to!!.optString(it) },
            state = o.optString("state"),
            error = o.optString("last_error"),
            dismissable = o.optBoolean("dismissable"),
        )
    }
}

/**
 * Unsent mail of the open account: queued, sending or failed sends. A sync
 * retries everything still deliverable (the core's `flush_outbox`); a dead
 * row — no bytes left, or out of retries — can only be forgotten here,
 * after a confirm, and the mail resent by hand. Re-reads whenever the
 * outbox counts move or a sync ends.
 */
@Composable
fun OutboxScreen(state: MailState) {
    val scope = rememberCoroutineScope()
    val accountId = state.activeAccountId
    var entries by remember { mutableStateOf<List<OutboxEntry>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var forget by remember { mutableStateOf<OutboxEntry?>(null) }

    LaunchedEffect(accountId, reload, state.outboxPending, state.outboxFailed, state.outboxRetryable, state.syncing) {
        if (accountId < 0) {
            entries = emptyList()
            return@LaunchedEffect
        }
        withContext(Dispatchers.IO) { runCatching { parse(MailNative.outboxJson(accountId)) } }
            .onSuccess { entries = it }
            .onFailure { error = it.message ?: "Could not read the outbox" }
    }

    fun dismiss(e: OutboxEntry) {
        scope.launch {
            withContext(Dispatchers.IO) { runCatching { MailNative.dismissOutbox(accountId, e.id) } }
                .onFailure { error = it.message ?: "Could not forget the entry" }
            state.refreshOutbox()
            reload++
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
            Text(
                "Mail waiting to send, or sends that failed. Syncing retries everything still deliverable.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 4.dp)) }
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
                    OutboxRow(e, onForget = { forget = e })
                    HorizontalDivider()
                }
            }
        }
    }

    forget?.let { e ->
        AlertDialog(
            onDismissRequest = { forget = null },
            title = { Text("Forget this entry?") },
            text = { Text("“${e.subject}” is not sent and leaves the outbox. Send it again by hand if it is still needed.") },
            confirmButton = {
                Button(
                    onClick = {
                        forget = null
                        dismiss(e)
                    },
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.error,
                        contentColor = MaterialTheme.colorScheme.onError,
                    ),
                ) { Text("Forget") }
            },
            dismissButton = { TextButton(onClick = { forget = null }) { Text("Cancel") } },
        )
    }
}

@Composable
private fun OutboxRow(e: OutboxEntry, onForget: () -> Unit) {
    Row(
        verticalAlignment = Alignment.Top,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxWidth().padding(start = 16.dp, top = 8.dp, bottom = 8.dp, end = 4.dp),
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                e.subject,
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.Bold,
                maxLines = 1,
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
            if (e.state.isNotEmpty()) {
                Text(e.state, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            if (e.error.isNotEmpty()) {
                Text(e.error, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
        }
        // Never a row being sent: forgetting it would not stop the mail.
        IconButton(onClick = onForget, enabled = e.dismissable) {
            Icon(painterResource(R.drawable.ic_close), "Forget this entry")
        }
    }
}
