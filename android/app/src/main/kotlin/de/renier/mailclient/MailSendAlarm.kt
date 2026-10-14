package de.renier.mailclient

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log

// The scheduled-send alarm: one exact shot for the earliest waiting
// scheduled send across all accounts (the core's `sendDueInMs`, which also
// spaces out retries of a send that failed after its time). Firing only
// enqueues an outbox-only flush — never the inbox check, so quiet hours,
// check intervals and push/poll mode cannot hold back mail the user asked
// to send — and then re-arms from the next waiting row, so deliveries
// converge without polling.
//
// Why its own alarm: the interval poller ticks every N minutes (15 at
// least), which would send minutes late; this fires on the minute. Like
// MailAlarm it is exact when allowed, inexact-but-Doze-proof otherwise, and
// comes back after a reboot or update through MailAlarmReceiver.
object MailSendAlarm {
    const val ACTION_SEND = "de.renier.mailclient.SCHEDULED_SEND"
    private const val REQUEST_CODE = 1003

    // Backstop while a due row cannot go out (offline, no password): the
    // worker waits for a network, and the next shot retries.
    const val RETRY_MILLIS = 15 * 60_000L

    // Plan the next shot from the waiting rows now. Database read only.
    // Called after every schedule change, on start, on boot/update, and by
    // the workers after each run (a delivery changed what waits).
    //
    // `flushIfDue`: a due row enqueues the flush right away. The send worker
    // itself passes false — it just ran that flush, and enqueueing again
    // would start the next worker the moment this one ends, over and over
    // while the row stays due — and gets the backstop after `retryMillis`.
    fun refresh(context: Context, flushIfDue: Boolean = true, retryMillis: Long = RETRY_MILLIS) {
        val inMs = try {
            MailNative.ensureInit(context)
            MailNative.sendDueInMs()
        } catch (e: RuntimeException) {
            Log.w("mailclient", "send alarm: next due unavailable", e)
            return
        }
        if (inMs < 0) {
            cancel(context)
            return
        }
        val now = System.currentTimeMillis()
        val at = if (inMs == 0L) {
            // Due (the phone was off at the time, or a flush could not
            // finish): flush now unless the caller just did — the worker
            // waits for a network when offline — and keep a backstop until
            // the row is gone.
            if (flushIfDue) MailCheckWorker.enqueueSendDue(context)
            now + retryMillis
        } else {
            now + inMs
        }
        val alarms = alarms(context)
        val exact = Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarms.canScheduleExactAlarms()
        if (exact) {
            alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pendingIntent(context))
        } else {
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pendingIntent(context))
        }
    }

    fun cancel(context: Context) {
        alarms(context).cancel(pendingIntent(context))
    }

    private fun pendingIntent(context: Context): PendingIntent = PendingIntent.getBroadcast(
        context,
        REQUEST_CODE,
        Intent(context, MailSendReceiver::class.java).setAction(ACTION_SEND),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    private fun alarms(context: Context) =
        context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
}

// A waiting row's time has come (or the backstop elapsed): plan again,
// which enqueues the outbox flush — never the inbox check — when it is due.
class MailSendReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != MailSendAlarm.ACTION_SEND) return
        MailSendAlarm.refresh(context)
    }
}
