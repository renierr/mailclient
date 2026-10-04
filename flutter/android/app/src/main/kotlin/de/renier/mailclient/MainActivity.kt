package de.renier.mailclient

import android.annotation.SuppressLint
import android.app.AlarmManager
import android.app.usage.UsageStatsManager
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    private var channel: MethodChannel? = null
    private var readerChannel: MethodChannel? = null

    // A notification tap that started or re-used the activity and that Dart
    // has not picked up yet.
    private var launchPayload: String? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        launchPayload = payloadOf(intent)
        val channel = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, POWER_CHANNEL)
        this.channel = channel
        channel.setMethodCallHandler { call, result ->
            when (call.method) {
                "status" -> result.success(powerStatus())
                "requestUnrestricted" -> result.success(requestUnrestricted())
                "exactAlarmStatus" -> result.success(canScheduleExactAlarms())
                "requestExactAlarm" -> result.success(requestExactAlarm())
                "schedule" -> {
                    MailSchedule.refresh(this)
                    result.success(true)
                }
                "showTestNotification" -> {
                    MailNotifier.showTest(this)
                    result.success(true)
                }
                "clearNotification" -> {
                    MailNotifier.clear(this)
                    result.success(true)
                }
                "takeLaunchPayload" -> {
                    result.success(launchPayload)
                    launchPayload = null
                }
                else -> result.notImplemented()
            }
        }
        MailNotifier.onMailChanged = {
            runOnUiThread { this.channel?.invokeMethod("mailChanged", null) }
        }
        // Experiment (branch experiment/native-reader): open one message in
        // the native ReaderActivity. Settings travel with the ids (never the
        // bodies — inline images exceed the Binder transaction limit); the
        // activity re-reads everything else over JNI. Composer, find-similar
        // and refresh requests come back over the same channel.
        val reader = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, READER_CHANNEL)
        readerChannel = reader
        reader.setMethodCallHandler { call, result ->
            when (call.method) {
                "open" -> {
                    @Suppress("UNCHECKED_CAST")
                    val args = call.arguments as? Map<String, Any?>
                    if (args == null) {
                        result.error("BAD_ARGS", "open needs an argument map", null)
                    } else {
                        try {
                            startActivity(ReaderActivity.openIntent(this, args))
                            result.success(true)
                        } catch (e: Exception) {
                            result.error("OPEN_FAILED", e.message, null)
                        }
                    }
                }
                else -> result.notImplemented()
            }
        }
        pendingReaderPayload?.let {
            reader.invokeMethod("onReaderAction", it)
            pendingReaderPayload = null
        }
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        MailNotifier.onMailChanged = null
        channel = null
        readerChannel = null
        super.cleanUpFlutterEngine(flutterEngine)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.getStringExtra(EXTRA_READER_PAYLOAD)?.let { payload ->
            val reader = readerChannel
            if (reader == null) {
                pendingReaderPayload = payload
            } else {
                reader.invokeMethod("onReaderAction", payload)
            }
        }
        val payload = payloadOf(intent) ?: return
        val channel = channel
        if (channel == null) {
            launchPayload = payload
        } else {
            channel.invokeMethod("openPayload", payload)
        }
    }

    override fun onResume() {
        super.onResume()
        MailNotifier.foreground = true
        // Experiment (branch experiment/native-reader): the native reader
        // mutated mail while covering us — reload the lists behind it.
        if (readerDirty) {
            readerDirty = false
            readerChannel?.invokeMethod("onReaderChanged", null)
        }
    }

    override fun onPause() {
        MailNotifier.foreground = false
        super.onPause()
    }

    private fun payloadOf(intent: Intent?): String? =
        intent?.takeIf { it.action == MailNotifier.ACTION_OPEN }
            ?.getStringExtra(MailNotifier.EXTRA_PAYLOAD)
            ?.takeIf { it.isNotEmpty() }

    // Whether Doze and App Standby may defer the background worker: the
    // battery-optimisation exemption plus the standby bucket Android put
    // the app in (0 when the API predates buckets).
    private fun powerStatus(): Map<String, Any> {
        val power = getSystemService(Context.POWER_SERVICE) as PowerManager
        val bucket = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            (getSystemService(Context.USAGE_STATS_SERVICE) as UsageStatsManager).appStandbyBucket
        } else {
            0
        }
        return mapOf(
            "unrestricted" to power.isIgnoringBatteryOptimizations(packageName),
            "standbyBucket" to bucket,
        )
    }

    // Ask Android to stop battery-optimising the app — the only way a
    // periodic worker keeps running on time while the phone sleeps. Falls
    // back to the settings list when the direct prompt is unavailable.
    @SuppressLint("BatteryLife")
    private fun requestUnrestricted(): Boolean {
        val direct = Intent(
            Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
            Uri.parse("package:$packageName"),
        )
        return try {
            startActivity(direct)
            true
        } catch (_: ActivityNotFoundException) {
            try {
                startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
                true
            } catch (_: ActivityNotFoundException) {
                false
            }
        }
    }

    companion object {
        const val POWER_CHANNEL = "mailclient/background_power"
        const val READER_CHANNEL = "mailclient/reader"

        // Experiment (branch experiment/native-reader): the native reader
        // asks Flutter for a shell flow (composer, find-similar) by
        // re-entering this activity with one of these set.
        const val ACTION_READER = "de.renier.mailclient.READER_ACTION"
        const val EXTRA_READER_PAYLOAD = "reader_payload"

        // Set by ReaderActivity when it changed mail; MainActivity reports
        // it to Dart on resume so the lists reload. Same process, no IPC.
        @Volatile var readerDirty = false
        var pendingReaderPayload: String? = null
    }

    // Whether the exact-alarm scheduler may fire at the exact minute.
    // Below Android 12 there is no such permission; since 14 it is denied
    // by default and the user grants it in the system settings.
    private fun canScheduleExactAlarms(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return true
        val alarms = getSystemService(Context.ALARM_SERVICE) as AlarmManager
        return alarms.canScheduleExactAlarms()
    }

    // Open the system's "Alarms & reminders" screen for this app.
    private fun requestExactAlarm(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return true
        val screen = Intent(
            Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM,
            Uri.parse("package:$packageName"),
        )
        return try {
            startActivity(screen)
            true
        } catch (_: ActivityNotFoundException) {
            false
        }
    }
}
