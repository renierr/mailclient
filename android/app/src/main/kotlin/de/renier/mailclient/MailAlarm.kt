package de.renier.mailclient

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.SystemClock
import androidx.lifecycle.LiveData
import androidx.lifecycle.Observer
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequest
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkInfo
import androidx.work.WorkManager

// The on-time background mail check: a self-rearming exact one-shot alarm
// (setExactAndAllowWhileIdle, so it fires in Doze) whose receiver hands the
// check to WorkManager as expedited work, run by MailCheckWorker (trigger
// "alarm") straight in the Rust core.
//
// Why a worker at all: a receiver gets seconds, a check can take longer.
// Doze defers plain jobs to its maintenance windows, so the check would run
// late however exactly the alarm fired; expedited work is exempt from that
// deferral, within a quota, and out of quota it still runs as ordinary
// work. Below Android 12 expedited work needs a foreground notification,
// so there it is ordinary work.
object MailAlarm {
    // WorkManager unique name of a pending alarm check.
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

    fun enqueueCheck(context: Context, trigger: String = "alarm", now: Boolean = false) {
        val request = OneTimeWorkRequest.Builder(MailCheckWorker::class.java)
            .setInputData(MailCheckWorker.input(trigger, now))
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

    /**
     * A check is queued or running — the one-shot alarm/tile checks and the
     * periodic poll — so the "Check mail" Quick Settings tile can show
     * itself busy. Blocks on WorkManager, so call it off the main thread.
     */
    fun checkRunning(context: Context): Boolean = runCatching {
        val work = WorkManager.getInstance(context)
        fun busy(name: String) =
            work.getWorkInfosForUniqueWork(name).get()
                .any { it.state == WorkInfo.State.RUNNING || it.state == WorkInfo.State.ENQUEUED }
        busy(CHECK_TASK) || busy(MailCheckWorker.PERIODIC)
    }.getOrDefault(false)

    /**
     * Call `onChange` on the main thread every time a check's work changes
     * state, and return what stops that again.
     *
     * A tile click does not collapse the shade on Android 12+, so the
     * "Check mail" tile stays visible while the check runs: without this it
     * would sit in its busy state until the shade is next opened. Main
     * thread only, like every LiveData call.
     */
    fun observeChecks(context: Context, onChange: (List<WorkInfo>) -> Unit): () -> Unit {
        val work = WorkManager.getInstance(context)
        val observed = listOf(CHECK_TASK, MailCheckWorker.PERIODIC).map { name ->
            val live = work.getWorkInfosForUniqueWorkLiveData(name)
            val observer = Observer<List<WorkInfo>> { onChange(it) }
            live.observeForever(observer)
            live to observer
        }
        return { observed.forEach { (live, observer) -> live.removeObserver(observer) } }
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

// Alarm shots, plus planning everything again after a reboot or an app
// update (both drop pending alarms or may): the poller, the push service and
// the quiet-hours replan alarm (MailSchedule).
class MailAlarmReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            MailAlarm.ACTION_FIRE -> {
                MailAlarm.enqueueCheck(context)
                MailAlarm.rearm(context)
            }
            Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED -> {
                if (MailSchedule.refresh(context) == null) {
                    // No plan from the core: keep what ran before.
                    MailAlarm.rearm(context)
                    MailPush.restore(context)
                }
            }
        }
    }
}
