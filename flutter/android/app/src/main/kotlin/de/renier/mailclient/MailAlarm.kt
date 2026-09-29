package de.renier.mailclient

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.SystemClock
import androidx.work.Constraints
import androidx.work.Data
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequest
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkManager
import dev.fluttercommunity.workmanager.BackgroundWorker
import dev.fluttercommunity.workmanager.UNIQUE_NAME_KEY

// The on-time background mail check: a self-rearming exact one-shot alarm
// (setExactAndAllowWhileIdle, so it fires in Doze) whose receiver hands the
// check to WorkManager as expedited work. The Dart side runs it through the
// same dispatcher as the periodic task (task name CHECK_TASK).
//
// Why native: an alarm plugin that runs its Dart callback through a
// JobIntentService queues a plain job, and Doze defers plain jobs to its
// maintenance windows, so the check ran late however exactly the alarm
// fired. Expedited work is exempt from that deferral, within a quota; out of
// quota it still runs as ordinary work. Below Android 12 expedited work
// needs a foreground notification the plugin's worker does not provide, so
// there it is ordinary work.
object MailAlarm {
    // Must match `alarmCheckTask` in background_sync.dart.
    private const val CHECK_TASK = "mail-alarm-check"
    const val ACTION_FIRE = "de.renier.mailclient.MAIL_ALARM"
    private const val PREFS = "mailclient_alarm"
    private const val KEY_MINUTES = "interval_minutes"
    private const val REQUEST_CODE = 1002

    // Store the interval and arm the next shot; 0 or less cancels.
    fun arm(context: Context, minutes: Int) {
        prefs(context).edit().putInt(KEY_MINUTES, minutes).apply()
        if (minutes > 0) schedule(context, minutes) else cancel(context)
    }

    fun cancel(context: Context) {
        prefs(context).edit().putInt(KEY_MINUTES, 0).apply()
        alarms(context).cancel(pendingIntent(context))
    }

    // Arm the next shot from the stored interval, if the alarm is on.
    fun rearm(context: Context) {
        val minutes = prefs(context).getInt(KEY_MINUTES, 0)
        if (minutes > 0) schedule(context, minutes)
    }

    fun enqueueCheck(context: Context) {
        val input = Data.Builder()
            .putString(BackgroundWorker.DART_TASK_KEY, CHECK_TASK)
            .putString(UNIQUE_NAME_KEY, CHECK_TASK)
            .build()
        val request = OneTimeWorkRequest.Builder(BackgroundWorker::class.java)
            .setInputData(input)
            .setConstraints(
                Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build(),
            )
            .apply {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
                }
            }
            .build()
        // KEEP: a check still waiting for the network is not stacked twice.
        WorkManager.getInstance(context)
            .enqueueUniqueWork(CHECK_TASK, ExistingWorkPolicy.KEEP, request)
    }

    // Exact when allowed; otherwise AllowWhileIdle still fires in Doze, just
    // not at the exact minute.
    private fun schedule(context: Context, minutes: Int) {
        val at = SystemClock.elapsedRealtime() + minutes * 60_000L
        val alarms = alarms(context)
        val exact = Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarms.canScheduleExactAlarms()
        if (exact) {
            alarms.setExactAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, at, pendingIntent(context))
        } else {
            alarms.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, at, pendingIntent(context))
        }
    }

    private fun pendingIntent(context: Context): PendingIntent = PendingIntent.getBroadcast(
        context,
        REQUEST_CODE,
        Intent(context, MailAlarmReceiver::class.java).setAction(ACTION_FIRE),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    private fun alarms(context: Context) =
        context.getSystemService(Context.ALARM_SERVICE) as AlarmManager

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}

// Alarm shots, plus re-arming after a reboot or an app update (both drop
// pending alarms or may).
class MailAlarmReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            MailAlarm.ACTION_FIRE -> {
                MailAlarm.enqueueCheck(context)
                MailAlarm.rearm(context)
            }
            Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED -> MailAlarm.rearm(context)
        }
    }
}
