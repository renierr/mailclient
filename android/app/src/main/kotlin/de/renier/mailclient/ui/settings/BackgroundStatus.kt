package de.renier.mailclient.ui.settings

import android.annotation.SuppressLint
import android.app.AlarmManager
import android.app.usage.UsageStatsManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import de.renier.mailclient.MailNative
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.strings
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

/** What Android lets the background checks do, read from the system. */
object BackgroundPower {
    fun unrestricted(context: Context): Boolean {
        val pm = context.getSystemService(PowerManager::class.java) ?: return true
        return pm.isIgnoringBatteryOptimizations(context.packageName)
    }

    /** The app's standby bucket, or null before Android 9. */
    fun standbyBucket(context: Context): Int? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) return null
        return context.getSystemService(UsageStatsManager::class.java)?.appStandbyBucket
    }

    fun exactAlarms(context: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
            context.getSystemService(AlarmManager::class.java)?.canScheduleExactAlarms() != false

    /** The system prompt to exempt the app from battery optimisation. */
    @SuppressLint("BatteryLife")
    fun requestUnrestricted(context: Context) {
        runCatching {
            context.startActivity(
                Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, Uri.parse("package:${context.packageName}"))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        }
    }

    /** The "Alarms & reminders" screen (Android 12+). */
    fun requestExactAlarms(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return
        runCatching {
            context.startActivity(
                Intent(Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM, Uri.parse("package:${context.packageName}"))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        }
    }

    /** The plan needs exact alarms: push's keep-alive or the alarm poller. */
    fun needsExact(plan: JSONObject): Boolean =
        plan.optBoolean("push") || (plan.optInt("poll_minutes") > 0 && plan.optString("poll_scheduler") == "alarm")
}

private class StatusData(
    val plan: JSONObject,
    val unrestricted: Boolean,
    val bucket: String,
    val exact: Boolean,
    val last: String,
    val history: List<String>,
)

/**
 * Whether the background checks may run on time (battery exemption,
 * standby bucket, exact alarms), what runs, and what the recent runs did —
 * for diagnosing missed notifications. Re-read on resume, so coming back
 * from a system prompt shows the new state.
 */
@Composable
fun BackgroundStatus() {
    val context = LocalContext.current
    var tick by remember { mutableIntStateOf(0) }
    var data by remember { mutableStateOf<StatusData?>(null) }
    var showHistory by remember { mutableStateOf(false) }

    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, e -> if (e == Lifecycle.Event.ON_RESUME) tick++ }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    LaunchedEffect(tick) {
        data = withContext(Dispatchers.IO) {
            runCatching {
                val lines = JSONObject(MailNative.backgroundRunLines())
                val history = lines.optJSONArray("history")
                StatusData(
                    plan = JSONObject(MailNative.backgroundPlanJson()),
                    unrestricted = BackgroundPower.unrestricted(context),
                    bucket = BackgroundPower.standbyBucket(context)?.let { MailNative.limitingBucket(it) }.orEmpty(),
                    exact = BackgroundPower.exactAlarms(context),
                    last = lines.optString("last"),
                    history = history.strings(),
                )
            }.getOrNull()
        }
    }

    val d = data ?: return
    val plan = d.plan
    val push = plan.optBoolean("push")
    val polledByAlarm = plan.optInt("poll_minutes") > 0 && plan.optString("poll_scheduler") == "alarm"
    val quiet = plan.optInt("quiet_accounts")
    Column {
        SettingHeading("Background checks")
        if (!plan.optBoolean("any")) {
            StatusLine(R.drawable.ic_sync, "Every account checks manually: nothing runs in the background.")
        }
        if (push) {
            StatusLine(
                R.drawable.ic_sync,
                "Push: the server announces new mail as it arrives. The \"Mail monitor\" notification Android " +
                    "requires for it can be turned off in the system notification settings.",
            )
        }
        if (plan.optInt("poll_minutes") > 0) {
            StatusLine(
                R.drawable.ic_sync,
                if (polledByAlarm) {
                    "On-time alarm: checks fire in standby too."
                } else {
                    "Battery-saving worker: standby may delay checks until the phone is unlocked."
                },
            )
        }
        if (quiet > 0) {
            StatusLine(
                R.drawable.ic_info,
                if (quiet == 1) {
                    "One account is in its quiet hours: no checks for it right now."
                } else {
                    "$quiet accounts are in their quiet hours: no checks for them right now."
                },
            )
        }
        if ((push || polledByAlarm) && !d.exact) {
            StatusLine(
                R.drawable.ic_report,
                if (push) {
                    "Exact alarms are not allowed: in standby the push keep-alive may run late and connections drop."
                } else {
                    "Exact alarms are not allowed: the alarm still fires in standby, just not at the exact minute."
                },
                error = true,
            )
            FilledTonalButton(onClick = { BackgroundPower.requestExactAlarms(context) }) { Text("Allow exact alarms") }
        }
        StatusLine(
            R.drawable.ic_info,
            if (d.unrestricted) {
                "Battery use is unrestricted, so checks keep running while the phone sleeps."
            } else {
                "Battery optimisation is on: while the phone sleeps, Android may postpone checks by hours."
            },
            error = !d.unrestricted,
        )
        if (!d.unrestricted && d.bucket.isNotEmpty()) {
            StatusLine(
                R.drawable.ic_report,
                "Android rates the app as \"${d.bucket}\", which limits checks further.",
                error = true,
            )
        }
        if (!d.unrestricted) {
            FilledTonalButton(onClick = { BackgroundPower.requestUnrestricted(context) }) { Text("Allow background use") }
        }
        StatusLine(R.drawable.ic_event, d.last)
        if (d.history.size > 1) {
            TextButton(onClick = { showHistory = !showHistory }) {
                Text(if (showHistory) "Hide recent checks" else "Recent checks")
            }
            if (showHistory) {
                for (line in d.history) {
                    Text(
                        line,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(start = 26.dp, bottom = 2.dp),
                    )
                }
            }
        }
    }
}
