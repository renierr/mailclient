package de.renier.mailclient.ui.settings

import android.Manifest
import android.app.TimePickerDialog
import android.text.format.DateFormat
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.PhoneContacts
import de.renier.mailclient.R
import kotlinx.coroutines.launch
import org.json.JSONObject

private const val INTERVAL = "sync_interval_minutes"
private const val PUSH = "push_enabled"
private const val QUIET = "quiet_hours_enabled"
private const val QUIET_START = "quiet_hours_start"
private const val QUIET_END = "quiet_hours_end"
private const val PHONE_CONTACTS = "suggest_phone_contacts"

/**
 * The two ends of a quiet-hours window, each a button opening the system
 * time picker. Values are stored `HH:MM`; the core reads and writes them
 * (`quietTime`, `quietTimeAt`), an unreadable one shows [fallback].
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun QuietHoursTimes(start: String, end: String, fallbackStart: String, fallbackEnd: String, onStart: (String) -> Unit, onEnd: (String) -> Unit) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.Center,
        modifier = Modifier.fillMaxWidth().padding(bottom = 6.dp),
    ) {
        TimeButton("From", start, fallbackStart, onStart)
        TimeButton("to", end, fallbackEnd, onEnd)
    }
}

@Composable
private fun TimeButton(label: String, value: String, fallback: String, onPick: (String) -> Unit) {
    val context = LocalContext.current
    fun parts(text: String): Pair<Int, Int>? = runCatching {
        val json = MailNative.quietTime(text)
        if (json.isEmpty()) null else JSONObject(json).let { it.getInt("hour") to it.getInt("minute") }
    }.getOrNull()
    val (hour, minute) = remember(value, fallback) { parts(value) ?: parts(fallback) ?: (0 to 0) }
    val is24 = DateFormat.is24HourFormat(context)
    val shown = if (is24) {
        "%02d:%02d".format(hour, minute)
    } else {
        "%d:%02d %s".format(if (hour % 12 == 0) 12 else hour % 12, minute, if (hour < 12) "AM" else "PM")
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(label, modifier = Modifier.padding(end = 6.dp))
        OutlinedButton(onClick = {
            TimePickerDialog(context, { _, h, m ->
                val stored = MailNative.quietTimeAt(h, m)
                if (stored.isNotEmpty()) onPick(stored)
            }, hour, minute, is24).show()
        }) { Text(shown) }
    }
}

/**
 * The app-wide sync settings: what every account uses unless it sets its
 * own (see [AccountSyncSettings]).
 */
@Composable
fun GlobalSyncSettings(draft: SettingsDraft, choices: JSONObject, onTestNotification: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    SettingSwitch("Save a copy of sent mail in Sent", draft.flag("sent_copy_enabled"), { draft.setFlag("sent_copy_enabled", it) })
    SettingSwitch("Suggest recipients from sent mail", draft.flag("collect_sent_contacts"), { draft.setFlag("collect_sent_contacts", it) })
    // The phone's own list only helps with READ_CONTACTS, so the switch
    // stays off until that is granted; denying it leaves nothing behind.
    val contactsPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        draft.setFlag(PHONE_CONTACTS, granted)
        scope.launch {
            if (granted) PhoneContacts.loadOnce(context) else PhoneContacts.dropSnapshot()
        }
    }
    SettingSwitch(
        title = "Suggest recipients from the phone's contacts",
        checked = draft.flag(PHONE_CONTACTS),
        help = "Contacts saved on this phone join the collected ones and are suggested first.",
        onChange = { on: Boolean ->
            if (on && !PhoneContacts.granted(context)) {
                contactsPermission.launch(Manifest.permission.READ_CONTACTS)
                return@SettingSwitch
            }
            draft.setFlag(PHONE_CONTACTS, on)
            scope.launch {
                if (on) PhoneContacts.loadOnce(context) else PhoneContacts.dropSnapshot()
            }
        },
    )
    SettingChoice(
        title = "Check for new mail",
        value = draft[INTERVAL],
        options = draft.options(INTERVAL),
        label = { SettingLabels.of(INTERVAL, it) },
        onChange = { draft[INTERVAL] = it },
        help = "Battery-saving checks run at most every 15 minutes. With push, any interval but Manually turns it on.",
    )
    SettingChoice(
        title = "Background check method",
        value = draft["background_scheduler"],
        options = draft.options("background_scheduler"),
        label = { SettingLabels.of("background_scheduler", it) },
        onChange = { draft["background_scheduler"] = it },
        help = "Push: the server announces new mail as it arrives. The on-time alarm checks at the interval, " +
            "in standby too. Each account can choose push or interval checks itself.",
    )
    SettingSwitch(
        "Quiet hours",
        draft.flag(QUIET),
        { draft.setFlag(QUIET, it) },
        help = "No background checks or push between these times. Opening the app or syncing by hand still " +
            "checks. Each account can choose its own.",
    )
    if (draft.flag(QUIET)) {
        QuietHoursTimes(
            start = draft[QUIET_START],
            end = draft[QUIET_END],
            fallbackStart = defaultOf(choices, QUIET_START),
            fallbackEnd = defaultOf(choices, QUIET_END),
            onStart = { draft[QUIET_START] = it },
            onEnd = { draft[QUIET_END] = it },
        )
    }
    SettingSwitch("Show notifications for new mail", draft.flag("notifications_enabled"), { draft.setFlag("notifications_enabled", it) })
    SettingChoice(
        title = "Notification button",
        value = draft["notification_action"],
        options = draft.options("notification_action"),
        label = { SettingLabels.of("notification_action", it) },
        onChange = { draft["notification_action"] = it },
        help = "A new mail's notification offers Mark read, Reply and this. Neither can be undone from the notification.",
    )
    OutlinedButton(onClick = onTestNotification, modifier = Modifier.padding(top = 4.dp)) {
        Icon(painterResource(R.drawable.ic_info), null, Modifier.size(18.dp))
        Text("Send test notification", modifier = Modifier.padding(start = 8.dp))
    }
}

fun defaultOf(choices: JSONObject, key: String): String = rawOf(choices.optJSONObject(key)?.opt("default"))

/**
 * One account's sync settings: every row offers "Default (…)", which
 * inherits the app-wide value in [defaults], or a value of its own.
 */
@Composable
fun AccountSyncSettings(defaults: SettingsDraft, account: AccountDraft, choices: JSONObject) {
    fun onOff(on: Boolean) = if (on) "On" else "Off"
    fun interval(v: String) = SettingLabels.of(INTERVAL, v)

    SettingChoice(
        title = "Check for new mail",
        value = account[INTERVAL],
        options = listOf("") + defaults.options(INTERVAL),
        label = { if (it.isEmpty()) "Default (${interval(defaults[INTERVAL])})" else interval(it) },
        onChange = { account[INTERVAL] = it },
    )
    val pushByDefault = defaults["background_scheduler"] == "push"
    SettingChoice(
        title = "Push (IMAP IDLE)",
        value = account[PUSH],
        options = listOf("", "1", "0"),
        label = {
            when (it) {
                "1" -> "Push"
                "0" -> "Check at the interval"
                else -> "Default (${if (pushByDefault) "push" else "check at the interval"})"
            }
        },
        onChange = { account[PUSH] = it },
        help = "Servers that send \"still here\" every few minutes wake the phone each time; checking at the " +
            "interval saves battery.",
    )
    val usesPush = account[PUSH].let { if (it.isEmpty()) pushByDefault else it == "1" }
    val heartbeat = account.heartbeatSecs
    if (usesPush && heartbeat != null) {
        val gap = runCatching { MailNative.heartbeatGap(heartbeat) }.getOrDefault("")
        StatusLine(
            R.drawable.ic_info,
            "This server sends \"still here\" $gap while push waits, waking the phone each time. If battery " +
                "matters, set Push to \"Check at the interval\".",
            error = true,
        )
    }

    val inherited = if (defaults.flag(QUIET)) "${defaults[QUIET_START]}–${defaults[QUIET_END]}" else "Off"
    SettingChoice(
        title = "Quiet hours",
        value = account[QUIET],
        options = listOf("", "1", "0"),
        label = {
            when (it) {
                "1" -> "On"
                "0" -> "Off"
                else -> "Default ($inherited)"
            }
        },
        onChange = {
            account[QUIET] = it
            // Own times only count with "On"; leaving it drops them.
            if (it != "1") {
                account[QUIET_START] = ""
                account[QUIET_END] = ""
            }
        },
        help = "No background checks or push between these times. Opening the app or syncing by hand still checks.",
    )
    if (account[QUIET] == "1") {
        QuietHoursTimes(
            start = account[QUIET_START].ifEmpty { defaults[QUIET_START] },
            end = account[QUIET_END].ifEmpty { defaults[QUIET_END] },
            fallbackStart = defaultOf(choices, QUIET_START),
            fallbackEnd = defaultOf(choices, QUIET_END),
            onStart = { account[QUIET_START] = it },
            onEnd = { account[QUIET_END] = it },
        )
    }

    for ((key, title) in listOf(
        "sent_copy_enabled" to "Save a copy of sent mail in Sent",
        "collect_sent_contacts" to "Suggest recipients from sent mail",
        "notifications_enabled" to "Show notifications for new mail",
    )) {
        SettingChoice(
            title = title,
            value = account[key],
            options = listOf("", "1", "0"),
            label = {
                when (it) {
                    "1" -> "On"
                    "0" -> "Off"
                    else -> "Default (${onOff(defaults.flag(key))})"
                }
            },
            onChange = { account[key] = it },
        )
    }
    Text(
        "Accounts use the app-wide settings unless they choose their own here.",
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(top = 8.dp),
    )
}
