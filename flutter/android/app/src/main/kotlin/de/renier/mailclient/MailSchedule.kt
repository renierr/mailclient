package de.renier.mailclient

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log
import org.json.JSONObject

// Which background mechanisms check for mail while the app is closed, as
// the Rust core planned them from every account's settings
// (mailcore::sync::background::schedule): the push service when any account
// uses push, and one poller (WorkManager or the exact alarm) every `minutes`
// for the polled accounts. Each poller tick only checks the accounts that
// are due, so the two never check the same account. Everything not in the
// plan is stopped.
//
// Accounts in their quiet hours are left out of the plan, so at night
// nothing may run at all. The plan names the next time a quiet window starts
// or ends (`replan_at`); a wake-up alarm then plans again, and nothing else
// wakes the phone in between.
object MailSchedule {
    const val ACTION_REPLAN = "de.renier.mailclient.REPLAN"
    private const val REQUEST_CODE = 1004

    // Plan from the current settings and run it; returns the plan, or null
    // when the core could not make one. Database only, no network.
    fun refresh(context: Context): JSONObject? {
        val plan = try {
            MailNative.ensureInit(context)
            JSONObject(MailNative.backgroundPlan())
        } catch (e: RuntimeException) {
            Log.w("mailclient", "background plan unavailable", e)
            return null
        }
        apply(
            context,
            plan.optBoolean("push"),
            plan.optString("poll_scheduler", "workmanager"),
            plan.optInt("poll_minutes"),
        )
        val replanAt = if (plan.isNull("replan_at")) 0L else plan.optLong("replan_at")
        if (replanAt > 0) armReplan(context, replanAt * 1000L) else cancelReplan(context)
        return plan
    }

    // A quiet window started or ended: plan again, and check the polled
    // accounts at once rather than a full interval later. Accounts still
    // quiet (or not yet due) are skipped by the check itself.
    fun replan(context: Context) {
        val plan = refresh(context) ?: return
        if (plan.optInt("poll_minutes") > 0) MailAlarm.enqueueCheck(context, "quiet-end")
    }

    private fun apply(context: Context, push: Boolean, pollMode: String, minutes: Int) {
        if (push) MailPush.enable(context) else MailPush.disable(context)
        val poll = minutes > 0
        if (poll && pollMode == "alarm") MailAlarm.arm(context, minutes) else MailAlarm.cancel(context)
        if (poll && pollMode != "alarm") {
            MailCheckWorker.schedulePeriodic(context, minutes)
        } else {
            MailCheckWorker.cancelPeriodic(context)
        }
    }

    // Wall-clock alarm: the boundary is a local time of day. Exact when
    // allowed; otherwise AllowWhileIdle still fires in Doze, a little late.
    private fun armReplan(context: Context, atMillis: Long) {
        val alarms = alarms(context)
        val exact = Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarms.canScheduleExactAlarms()
        if (exact) {
            alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, atMillis, pendingIntent(context))
        } else {
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, atMillis, pendingIntent(context))
        }
    }

    private fun cancelReplan(context: Context) {
        alarms(context).cancel(pendingIntent(context))
    }

    private fun pendingIntent(context: Context): PendingIntent = PendingIntent.getBroadcast(
        context,
        REQUEST_CODE,
        Intent(context, MailScheduleReceiver::class.java).setAction(ACTION_REPLAN),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    private fun alarms(context: Context) =
        context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
}

// The replan alarm, plus the clock moving under it: a new time zone or a
// changed time shifts every quiet window.
class MailScheduleReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            MailSchedule.ACTION_REPLAN -> MailSchedule.replan(context)
            Intent.ACTION_TIMEZONE_CHANGED, Intent.ACTION_TIME_CHANGED -> {
                MailSchedule.refresh(context)
            }
        }
    }
}
