package de.renier.mailclient

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import android.util.Log
import org.json.JSONObject

// The new-mail notification, for every scheduler (worker, alarm, push).
// What to show is decided in Rust (sync::background::notify); this reads
// back the Android state the plan needs, posts it and commits it.
object MailNotifier {
    // Channel and id shared with the notifications the Dart side posted
    // before the checks moved here, so an update replaces rather than adds.
    const val CHANNEL_ID = "mail_new"
    private const val CHANNEL_NAME = "New mail"
    const val NOTIFICATION_ID = 0

    const val ACTION_OPEN = "de.renier.mailclient.OPEN_MAIL"
    const val EXTRA_PAYLOAD = "payload"

    private const val TAG = "mailclient"

    // Whether MainActivity is on screen: new mail then shows in the list
    // instead of alerting (set from onResume/onPause).
    @Volatile var foreground = false

    // Told when a check saw new mail while the app is open, so the list
    // re-reads the cache. Set by MainActivity while its engine lives.
    @Volatile var onMailChanged: (() -> Unit)? = null

    // Post, update or clear the notification for a BackgroundReport, then
    // commit. A failed post leaves the marks uncommitted, so the next check
    // reports the same mail again.
    @Synchronized
    fun deliver(context: Context, report: String) {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        val planJson = MailNative.plan(report, manager.areNotificationsEnabled(), foreground, shownSignature(manager))
        val plan = JSONObject(planJson)
        try {
            when (plan.getString("action")) {
                "alert" -> post(context, manager, plan, silent = false)
                "update" -> post(context, manager, plan, silent = true)
                "clear" -> manager.cancel(NOTIFICATION_ID)
                "foreground" -> onMailChanged?.invoke()
            }
        } catch (e: RuntimeException) {
            Log.w(TAG, "new-mail notification failed", e)
            MailNative.recordOutcome(plan.optString("run"), "notification failed: ${e.message}")
            return
        }
        MailNative.commit(planJson)
    }

    // A sample with every display option, for Settings' test button.
    fun showTest(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        val plan = JSONObject()
            .put("title", "Mailclient Test")
            .put("body", "Test notification — tap to open the app")
            .put("count", 1)
            .put("payload", "")
        post(context, manager, plan, silent = false)
    }

    fun clear(context: Context) {
        context.getSystemService(NotificationManager::class.java)?.cancel(NOTIFICATION_ID)
    }

    // Title and body of the notification on screen, as the plan compares
    // them; null when there is none (never posted, or swiped away).
    private fun shownSignature(manager: NotificationManager): String? {
        val mine = manager.activeNotifications.firstOrNull { it.id == NOTIFICATION_ID } ?: return null
        val extras = mine.notification.extras
        val title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString() ?: ""
        val body = extras.getCharSequence(Notification.EXTRA_TEXT)?.toString() ?: ""
        return "$title\n$body"
    }

    private fun post(context: Context, manager: NotificationManager, plan: JSONObject, silent: Boolean) {
        ensureChannel(manager)
        val title = plan.optString("title")
        val body = plan.optString("body")
        val builder = builder(context)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle(title)
            .setContentText(body)
            .setNumber(plan.optInt("count"))
            .setAutoCancel(true)
            .setOnlyAlertOnce(silent)
            .setCategory(Notification.CATEGORY_EMAIL)
            .setContentIntent(openIntent(context, plan.optString("payload")))
        val lines = plan.optJSONArray("lines")
        if (lines != null && lines.length() > 0) {
            val style = Notification.InboxStyle().setBigContentTitle(title)
            for (i in 0 until lines.length()) style.addLine(lines.getString(i))
            if (!plan.isNull("summary")) style.setSummaryText(plan.optString("summary"))
            builder.setStyle(style)
        }
        if (silent) builder.muted()
        manager.notify(NOTIFICATION_ID, builder.build())
    }

    private fun Notification.Builder.muted(): Notification.Builder {
        // Below Android 8 the channel cannot mute a single post; no sound,
        // no vibration is the same thing there.
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            @Suppress("DEPRECATION")
            setDefaults(0).setSound(null).setVibrate(null)
        }
        return this
    }

    @Suppress("DEPRECATION")
    private fun builder(context: Context): Notification.Builder =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(context, CHANNEL_ID)
        } else {
            Notification.Builder(context)
                .setPriority(Notification.PRIORITY_HIGH)
                .setDefaults(Notification.DEFAULT_ALL)
        }

    private fun openIntent(context: Context, payload: String): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .setAction(ACTION_OPEN)
            .putExtra(EXTRA_PAYLOAD, payload)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        return PendingIntent.getActivity(
            context,
            0,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun ensureChannel(manager: NotificationManager) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        if (manager.getNotificationChannel(CHANNEL_ID) != null) return
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_ID, CHANNEL_NAME, NotificationManager.IMPORTANCE_HIGH).apply {
                description = "Alerts for mail that arrived while the app was closed"
            },
        )
    }
}
