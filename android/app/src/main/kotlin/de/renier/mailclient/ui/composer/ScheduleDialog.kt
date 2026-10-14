package de.renier.mailclient.ui.composer

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SelectableDates
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TimeInput
import androidx.compose.material3.TimePicker
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.material3.rememberTimePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.ui.common.isShortScreen
import kotlinx.coroutines.delay
import java.time.Instant
import java.time.LocalDate
import java.time.LocalTime
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

// Pick the date and time a mail goes out: a calendar and a clock, opening
// on the current date and time. The result is a UTC instant (ISO-8601,
// "…Z"), stored in the outbox as `send_at` and delivered by the alarm past
// it. Device-local by design: the dialog says the phone must stay on and
// online. Whether the picked time is valid, and why not, comes from the
// core (`MailNative.sendAtError`), the same check the Qt dialog uses.
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
    val context = LocalContext.current
    val zone = ZoneId.systemDefault()
    // The held instant when rescheduling, else right now (still invalid
    // until moved into the future — the core enforces the lead time).
    val start = remember { (parseInstant(initialSendAtUtc) ?: Instant.now()).atZone(zone) }
    // Selection as local wall-clock parts; the UTC instant derives from it.
    var date by remember { mutableStateOf(start.toLocalDate()) }
    var hour by remember { mutableIntStateOf(start.hour) }
    var minute by remember { mutableIntStateOf(start.minute) }
    var pickingDate by remember { mutableStateOf(false) }
    var pickingTime by remember { mutableStateOf(false) }
    // Mail a delivery note to the own address once this send goes out.
    var notifySelf by remember { mutableStateOf(initialNotifySelf) }
    // Re-checks a picked time that slips into the past while the dialog
    // stays open, without the user touching anything.
    var nowTick by remember { mutableIntStateOf(0) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(15_000)
            nowTick++
        }
    }

    val selected = date.atTime(LocalTime.of(hour, minute)).atZone(zone).toInstant()
    val sendAtUtc = selected.toString()
    val timeError = remember(sendAtUtc, nowTick) {
        runCatching { MailNative.sendAtError(sendAtUtc) }.getOrDefault("")
    }
    val valid = timeError.isEmpty()

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Send later") },
        text = {
            // Scrolls on short screens and large text, so the toggle and
            // its note stay reachable.
            Column(
                verticalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.verticalScroll(rememberScrollState()),
            ) {
                Text(
                    "The mail waits in this phone's outbox and goes out at its time — the phone must be on and online then.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = { pickingDate = true }, modifier = Modifier.weight(1f)) {
                        Text(date.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)))
                    }
                    OutlinedButton(onClick = { pickingTime = true }, modifier = Modifier.weight(1f)) {
                        Text(LocalTime.of(hour, minute).format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT)))
                    }
                }
                Text(
                    if (valid) sendsAtText(selected) else timeError,
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
            TextButton(onClick = { onConfirm(sendAtUtc, notifySelf) }, enabled = valid) {
                Text("Schedule send")
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text("Cancel") }
        },
    )

    if (pickingDate) {
        // Material3's DatePicker counts in UTC days: hand it the picked
        // local date at UTC midnight and read its answer back the same way,
        // or zones east and west of UTC land a day off.
        val today = LocalDate.now(zone)
        val state = rememberDatePickerState(
            initialSelectedDateMillis = date.atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli(),
            selectableDates = object : SelectableDates {
                override fun isSelectableDate(utcTimeMillis: Long): Boolean =
                    !utcDay(utcTimeMillis).isBefore(today)

                override fun isSelectableYear(year: Int): Boolean = year >= today.year
            },
        )
        DatePickerDialog(
            onDismissRequest = { pickingDate = false },
            confirmButton = {
                TextButton(
                    onClick = {
                        state.selectedDateMillis?.let { date = utcDay(it) }
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
        val state = rememberTimePickerState(
            initialHour = hour,
            initialMinute = minute,
            is24Hour = android.text.format.DateFormat.is24HourFormat(context),
        )
        AlertDialog(
            onDismissRequest = { pickingTime = false },
            title = { Text("Time") },
            text = {
                // The clock dial needs more height than a short screen has.
                if (isShortScreen()) {
                    TimeInput(state)
                } else {
                    TimePicker(state, modifier = Modifier.fillMaxWidth())
                }
            },
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

private fun parseInstant(raw: String): Instant? =
    if (raw.isEmpty()) null else runCatching { Instant.parse(raw) }.getOrNull()

private fun utcDay(utcMillis: Long): LocalDate =
    Instant.ofEpochMilli(utcMillis).atZone(ZoneOffset.UTC).toLocalDate()

private fun sendsAtText(at: Instant): String {
    val local = at.atZone(ZoneId.systemDefault())
    return "Sends ${local.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL))} at " +
        local.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
}

/** "Next try Tuesday, 14 October 2026 at 08:15" for an outbox row's UTC `retry_at`; "" when unreadable. */
fun nextTryLabel(retryAtUtc: String): String = parseInstant(retryAtUtc)?.let {
    val local = it.atZone(ZoneId.systemDefault())
    "Next try ${local.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL))} at " +
        local.format(DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT))
}.orEmpty()

/** "Sends Tuesday, 14 October 2026 at 08:00" for an outbox row's UTC `send_at`; "" when unreadable. */
fun sendsAtLabel(sendAtUtc: String): String = parseInstant(sendAtUtc)?.let { sendsAtText(it) }.orEmpty()
