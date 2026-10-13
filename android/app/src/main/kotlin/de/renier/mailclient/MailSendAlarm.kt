package de.renier.mailclient

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log
import org.json.JSONObject
import java.text.SimpleDateFormat
import java.util.Locale
import java.util.TimeZone

// The scheduled-send alarm: one exact shot for the earliest held `scheduled`
// outbox row across all accounts (the core's `nextSendDue`). Firing only
// enqueues a MailCheckWorker check — whose sync flushes due rows — and then
// re-arms from the next held row, so deliveries converge without polling.
//
// Why its own alarm: the interval poller ticks every N minutes (15 at
// least), which would send minutes late; this fires on the minute. Like
// MailAlarm it is exact when allowed, inexact-but-Doze-proof otherwise, and
// comes back after a reboot or update through MailAlarmReceiver.
object MailSendAlarm {
    const val ACTION_SEND = "de.renier.mailclient.SCHEDULED_SEND"
    private const val REQUEST_CODE = 1003

    // Backstop while a due row cannot go out (offline): the worker waits for
    // a network, and the next shot retries.
    private const val RETRY_MILLIS = 15 * 60_000L

    // Plan the next shot from the held rows now. Database read only.
    // Called after every schedule change, on start, on boot/update, and by
    // the worker after each check (a delivery changed what is held).
    fun refresh(context: Context) {
        val due = try {
            MailNative.ensureInit(context)
            JSONObject(MailNative.nextSendDue())
        } catch (e: RuntimeException) {
            Log.w("mailclient", "send alarm: next due unavailable", e)
            return
        }
        val at = due.optString("send_at")
        if (at.isEmpty()) {
            cancel(context)
            return
        }
        val millis = parseUtc(at)
        if (millis == null) {
            Log.w("mailclient", "send alarm: unreadable send_at")
            cancel(context)
            return
        }
        armAt(context, millis)
    }

    fun cancel(context: Context) {
        alarms(context).cancel(pendingIntent(context))
    }

    private fun armAt(context: Context, atMillis: Long) {
        // Past-due (the phone was off at the time): check now — the worker
        // delivers when online and waits for a network when not — and keep
        // the backstop until the row is gone.
        if (atMillis <= System.currentTimeMillis()) {
            MailAlarm.enqueueCheck(context, "scheduled-send")
        }
        val at = if (atMillis <= System.currentTimeMillis()) {
            System.currentTimeMillis() + RETRY_MILLIS
        } else {
            atMillis
        }
        val alarms = alarms(context)
        val exact = Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarms.canScheduleExactAlarms()
        if (exact) {
            alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pendingIntent(context))
        } else {
            alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, pendingIntent(context))
        }
    }

    private fun parseUtc(raw: String): Long? {
        for (p in arrayOf("yyyy-MM-dd'T'HH:mm:ssX", "yyyy-MM-dd'T'HH:mm:ss.SSSX")) {
            runCatching {
                val f = SimpleDateFormat(p, Locale.US)
                f.timeZone = TimeZone.getTimeZone("UTC")
                f.isLenient = false
                f.parse(raw)?.time
            }.getOrNull()?.let { return it }
        }
        return null
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

// A held row's time has come (or the backstop elapsed): run the same check
// the schedulers run — its sync flushes every due row — then plan again.
class MailSendReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != MailSendAlarm.ACTION_SEND) return
        MailAlarm.enqueueCheck(context, "scheduled-send")
        MailSendAlarm.refresh(context)
    }
}
