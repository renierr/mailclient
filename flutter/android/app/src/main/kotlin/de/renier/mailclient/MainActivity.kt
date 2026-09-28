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
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, POWER_CHANNEL)
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "status" -> result.success(powerStatus())
                    "requestUnrestricted" -> result.success(requestUnrestricted())
                    "exactAlarmStatus" -> result.success(canScheduleExactAlarms())
                    "requestExactAlarm" -> result.success(requestExactAlarm())
                    else -> result.notImplemented()
                }
            }
    }

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

    private companion object {
        const val POWER_CHANNEL = "mailclient/background_power"
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
