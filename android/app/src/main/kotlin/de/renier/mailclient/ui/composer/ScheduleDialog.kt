package de.renier.mailclient.ui.composer

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.text.DateFormat
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Date
import java.util.Locale
import java.util.TimeZone

// Pick the date and time a mail goes out: a calendar and a clock, opening
// on the current date and time. The result is a UTC instant
// ("yyyy-MM-dd'T'HH:mm:ss'Z'"), stored in the outbox as `send_at` and
// delivered by the next check past it. Device-local by design: the dialog
// says the phone must stay on and online.
//
// The receipt toggle opens on the setting's default (`receipt` from the
// core's `Receipts`), and what the user picks here travels back with the
// chosen time, overriding that default for this one send.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ScheduleDialog(
    initialSendAtUtc: String,
    initialNotifySelf: Boolean,
    onConfirm: (String, Boolean) -> Unit,
    onDismiss: () -> Unit,
    // Only the composer offers the choice: rescheduling a held row moves
    // its time and nothing else, so the row's own receipt flag stands and
    // the toggle would decide something it cannot carry out.
    showReceipt: Boolean = true,
) {
    // Selection as local wall-clock parts; the UTC instant derives from it.
    var day by remember { mutableStateOf(midnightOf(initialDay(initialSendAtUtc))) }
    var hour by remember { mutableStateOf(initialHour(initialSendAtUtc)) }
    var minute by remember { mutableStateOf(initialMinute(initialSendAtUtc)) }
    var pickingDate by remember { mutableStateOf(false) }
    var pickingTime by remember { mutableStateOf(false) }
    // Mail a delivery note to the own address once this send goes out.
    var notifySelf by remember { mutableStateOf(initialNotifySelf) }

    fun selectedMillis(): Long {
        val c = Calendar.getInstance()
        c.timeInMillis = day
        c.set(Calendar.HOUR_OF_DAY, hour)
        c.set(Calendar.MINUTE, minute)
        c.set(Calendar.SECOND, 0)
        c.set(Calendar.MILLISECOND, 0)
        return c.timeInMillis
    }

    val valid = selectedMillis() - System.currentTimeMillis() > LEAD_MILLIS
    val summary = if (valid) {
        val d = Date(selectedMillis())
        "Sends ${DateFormat.getDateInstance(DateFormat.FULL).format(d)} at " +
            "${DateFormat.getTimeInstance(DateFormat.SHORT).format(d)}"
    } else {
        "Pick a time at least a minute in the future"
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Send later") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    "The mail waits in this phone's outbox and goes out with the first check past its time — the phone must be on and online then.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = { pickingDate = true }, modifier = Modifier.weight(1f)) {
                        Text(DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(day)))
                    }
                    OutlinedButton(onClick = { pickingTime = true }, modifier = Modifier.weight(1f)) {
                        Text(DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(selectedMillis())))
                    }
                }
                Text(
                    summary,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (valid) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
                )
                if (showReceipt) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(checked = notifySelf, onCheckedChange = { notifySelf = it })
                        Text("Mail me a delivery receipt", style = MaterialTheme.typography.bodyMedium)
                    }
                    Text(
                        "When it goes out, this phone mails itself a note naming the subject, the time and how many recipients it had.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onConfirm(formatUtc(selectedMillis()), notifySelf) },
                enabled = valid,
            ) {
                Text("Schedule send")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )

    if (pickingDate) {
        val state = rememberDatePickerState(initialSelectedDateMillis = day)
        DatePickerDialog(
            onDismissRequest = { pickingDate = false },
            confirmButton = {
                TextButton(
                    onClick = {
                        state.selectedDateMillis?.let { day = midnightOf(it) }
                        pickingDate = false
                    },
                ) { Text("OK") }
            },
            dismissButton = {
                TextButton(onClick = { pickingDate = false }) { Text("Cancel") }
            },
        ) {
            DatePicker(state)
        }
    }
    if (pickingTime) {
        val state = rememberTimePickerState(initialHour = hour, initialMinute = minute, is24Hour = true)
        AlertDialog(
            onDismissRequest = { pickingTime = false },
            title = { Text("Time") },
            text = { TimePicker(state, modifier = Modifier.fillMaxWidth()) },
            confirmButton = {
                TextButton(
                    onClick = {
                        hour = state.hour
                        minute = state.minute
                        pickingTime = false
                    },
                ) { Text("OK") }
            },
            dismissButton = {
                TextButton(onClick = { pickingTime = false }) { Text("Cancel") }
            },
        )
    }
}

// The core only ever stores normalized UTC seconds ("...Z"), but parse
// leniently: offsets and millis read too.
private fun parseUtc(raw: String): Long? {
    if (raw.isEmpty()) return null
    val patterns = arrayOf("yyyy-MM-dd'T'HH:mm:ssX", "yyyy-MM-dd'T'HH:mm:ss.SSSX")
    for (p in patterns) {
        runCatching {
            val f = SimpleDateFormat(p, Locale.US)
            f.timeZone = TimeZone.getTimeZone("UTC")
            f.isLenient = false
            f.parse(raw)?.time
        }.getOrNull()?.let { return it }
    }
    return null
}

private fun midnightOf(millis: Long): Long {
    val c = Calendar.getInstance()
    c.timeInMillis = millis
    c.set(Calendar.HOUR_OF_DAY, 0)
    c.set(Calendar.MINUTE, 0)
    c.set(Calendar.SECOND, 0)
    c.set(Calendar.MILLISECOND, 0)
    return c.timeInMillis
}

// The held instant when rescheduling, else right now (still invalid until
// moved into the future — the confirm button enforces the lead time).
private fun initialDay(sendAtUtc: String): Long {
    parseUtc(sendAtUtc)?.let { return midnightOf(it) }
    return midnightOf(System.currentTimeMillis())
}

private fun initialHour(sendAtUtc: String): Int {
    parseUtc(sendAtUtc)?.let {
        val c = Calendar.getInstance()
        c.timeInMillis = it
        return c.get(Calendar.HOUR_OF_DAY)
    }
    return Calendar.getInstance().get(Calendar.HOUR_OF_DAY)
}

private fun initialMinute(sendAtUtc: String): Int {
    parseUtc(sendAtUtc)?.let {
        val c = Calendar.getInstance()
        c.timeInMillis = it
        return c.get(Calendar.MINUTE)
    }
    return Calendar.getInstance().get(Calendar.MINUTE)
}

/** UTC "yyyy-MM-dd'T'HH:mm:ss'Z'" for the core's `send_at`. */
fun formatUtc(millis: Long): String {
    val f = SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss'Z'", Locale.US)
    f.timeZone = TimeZone.getTimeZone("UTC")
    return f.format(Date(millis))
}

/** Local "Tuesday, 14 October, 08:00" words for an outbox row's `send_at`. */
fun formatSendAtLocal(sendAtUtc: String): String {
    val millis = parseUtc(sendAtUtc) ?: return ""
    val d = Date(millis)
    return "${DateFormat.getDateInstance(DateFormat.FULL).format(d)}, " +
        DateFormat.getTimeInstance(DateFormat.SHORT).format(d)
}

// Like the core's SCHEDULE_LEAD_SECS: anything sooner is an immediate send.
private const val LEAD_MILLIS = 60_000L
