package de.renier.mailclient

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.PowerManager
import android.os.SystemClock
import android.util.Log

// Push mail on/off, and the keep-alive alarm behind MailPushService.
//
// The Rust monitor's own timers stand still while the phone sleeps, so a
// wake-up alarm drives it instead: every KEEPALIVE_MINUTES it re-issues each
// IDLE (keeping the connection and the carrier's NAT mapping alive) and
// retries accounts that failed. Exact when allowed, so it keeps its cadence
// in Doze. The alarm also restarts the service if Android killed it.
object MailPush {
    const val ACTION_KEEPALIVE = "de.renier.mailclient.PUSH_KEEPALIVE"
    private const val KEEPALIVE_MINUTES = 15L
    private const val PREFS = "mailclient_push"
    private const val KEY_ENABLED = "enabled"
    private const val REQUEST_CODE = 1003

    fun isEnabled(context: Context): Boolean =
        prefs(context).getBoolean(KEY_ENABLED, false)

    fun enable(context: Context) {
        prefs(context).edit().putBoolean(KEY_ENABLED, true).apply()
        start(context)
    }

    fun disable(context: Context) {
        prefs(context).edit().putBoolean(KEY_ENABLED, false).apply()
        alarms(context).cancel(pendingIntent(context))
        context.stopService(Intent(context, MailPushService::class.java))
    }

    // After a reboot or an app update: start again if push was on.
    fun restore(context: Context) {
        if (isEnabled(context)) start(context)
    }

    // Start (or poke) the service. Android only lets a background app start
    // a foreground service from a few places — boot, app update, an exact
    // alarm, or while exempt from battery optimisation — so a refusal is
    // logged and the next keep-alive tries again.
    private fun start(context: Context) {
        val intent = Intent(context, MailPushService::class.java)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        } catch (e: RuntimeException) {
            Log.w("mailclient", "push service not started", e)
        }
        armKeepalive(context)
    }

    fun armKeepalive(context: Context) {
        val at = SystemClock.elapsedRealtime() + KEEPALIVE_MINUTES * 60_000L
        val alarms = alarms(context)
        val exact = Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarms.canScheduleExactAlarms()
        if (exact) {
            alarms.setExactAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, at, pendingIntent(context))
        } else {
            alarms.setAndAllowWhileIdle(AlarmManager.ELAPSED_REALTIME_WAKEUP, at, pendingIntent(context))
        }
    }

    // One keep-alive tick: poke the running monitor, or bring the service
    // back. The short wake lock bridges until the monitor reports busy.
    fun tick(context: Context) {
        if (!isEnabled(context)) return
        val power = context.getSystemService(Context.POWER_SERVICE) as PowerManager
        power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "mailclient:keepalive").acquire(10_000L)
        if (MailPushService.running) {
            MailNative.pushKeepalive()
            armKeepalive(context)
        } else {
            start(context)
        }
    }

    private fun pendingIntent(context: Context): PendingIntent = PendingIntent.getBroadcast(
        context,
        REQUEST_CODE,
        Intent(context, MailPushReceiver::class.java).setAction(ACTION_KEEPALIVE),
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
    )

    private fun alarms(context: Context) =
        context.getSystemService(Context.ALARM_SERVICE) as AlarmManager

    private fun prefs(context: Context) =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
}

class MailPushReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == MailPush.ACTION_KEEPALIVE) MailPush.tick(context)
    }
}
