package de.renier.mailclient

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.util.Log
import org.json.JSONObject

// The new-mail notifications, for every scheduler (worker, alarm, push) and
// for the "Mark read" buttons. What to show is decided in Rust
// (sync::background::notify): one group per account, a summary plus one
// child per mail, each told apart by its tag. This reads back what is on
// screen, cancels and posts what the plan says and commits it.
object MailNotifier {
    // Channel shared with the notifications the Dart side posted before the
    // checks moved here.
    const val CHANNEL_ID = "mail_new"
    private const val CHANNEL_NAME = "New mail"

    // Every mail notification is posted under this id with its own tag.
    // The untagged one is the single notification older versions posted.
    private const val NOTIFICATION_ID = 0

    const val ACTION_OPEN = "de.renier.mailclient.OPEN_MAIL"
    const val EXTRA_PAYLOAD = "payload"

    // Tags the plan uses (notify::OPEN_PAYLOAD_PREFIX, SUMMARY_TAG_PREFIX),
    // and the Settings test sample.
    private val TAG_PREFIXES = listOf("mail:", "account:")
    private const val TEST_TAG = "test"

    private const val TAG = "mailclient"

    // Whether MainActivity is on screen: new mail then shows in the list
    // instead of alerting (set from onResume/onPause).
    @Volatile var foreground = false

    // Told when mail changed while the app is open (a check saw new mail, a
    // notification button marked some read), so the list re-reads the
    // cache. Set by the Compose shell (MailShell) while it is composed.
    @Volatile var onMailChanged: (() -> Unit)? = null

    // Carry out the plan for a BackgroundReport, then commit. A failed post
    // leaves the marks uncommitted, so the next check reports the same mail
    // again.
    @Synchronized
    fun deliver(context: Context, report: String) {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        val planJson = MailNative.plan(report, manager.areNotificationsEnabled(), foreground, shown(manager).toString())
        val plan = JSONObject(planJson)
        try {
            manager.cancel(NOTIFICATION_ID)
            val cancel = plan.getJSONArray("cancel")
            for (i in 0 until cancel.length()) manager.cancel(cancel.getString(i), NOTIFICATION_ID)
            val post = plan.getJSONArray("post")
            if (post.length() > 0) ensureChannel(manager)
            for (i in 0 until post.length()) {
                val n = post.getJSONObject(i)
                manager.notify(n.getString("tag"), NOTIFICATION_ID, build(context, n))
            }
            if (plan.getString("action") == "foreground") onMailChanged?.invoke()
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
        ensureChannel(manager)
        val sample = JSONObject()
            .put("tag", TEST_TAG)
            // Ungrouped: a grouped child without its summary posts silently,
            // and the test is about hearing the alert.
            .put("summary", false)
            .put("title", "Mailclient Test")
            .put("body", "Test notification — tap to open the app")
            .put("big_text", "Test notification — tap to open the app\nExpanded, a mail shows the start of its text here.")
            .put("account", "Test")
            .put("redacted", "New message")
            .put("count", 1)
            .put("alert", true)
            .put("payload", "")
            .put("mark_read", "")
        manager.notify(TEST_TAG, NOTIFICATION_ID, build(context, sample))
    }

    // Remove every mail notification (the app was opened).
    fun clear(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        manager.cancel(NOTIFICATION_ID)
        for (n in manager.activeNotifications) {
            val tag = n.tag ?: continue
            if (n.id == NOTIFICATION_ID && (isMine(tag) || tag == TEST_TAG)) manager.cancel(tag, n.id)
        }
    }

    private fun isMine(tag: String) = TAG_PREFIXES.any { tag.startsWith(it) }

    // The mail notifications on screen, tag → title and body as read back;
    // the core turns them into the signatures it compares. Swiped-away ones
    // are gone from here.
    private fun shown(manager: NotificationManager): JSONObject {
        val out = JSONObject()
        for (n in manager.activeNotifications) {
            val tag = n.tag ?: continue
            if (n.id != NOTIFICATION_ID || !isMine(tag)) continue
            val extras = n.notification.extras
            val title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString() ?: ""
            val body = extras.getCharSequence(Notification.EXTRA_TEXT)?.toString() ?: ""
            out.put(tag, JSONObject().put("title", title).put("body", body))
        }
        return out
    }

    private fun build(context: Context, n: JSONObject): Notification {
        val tag = n.getString("tag")
        val title = n.optString("title")
        val body = n.optString("body")
        val account = n.optString("account")
        val summary = n.optBoolean("summary")
        val builder = builder(context)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle(title)
            .setContentText(body)
            .setSubText(account.ifEmpty { null })
            .setNumber(n.optInt("count"))
            .setAutoCancel(true)
            .setOnlyAlertOnce(!n.optBoolean("alert"))
            .setCategory(Notification.CATEGORY_EMAIL)
            .setVisibility(Notification.VISIBILITY_PRIVATE)
            .setPublicVersion(publicVersion(context, n))
            .setContentIntent(openIntent(context, tag, n.optString("payload")))
        if (!n.isNull("when")) builder.setWhen(n.getLong("when")).setShowWhen(true)
        // Only a real group gets group settings: `setGroup("")` still counts
        // as a group, and GROUP_ALERT_SUMMARY silences every grouped child.
        val group = n.optString("group")
        if (group.isNotEmpty()) {
            builder.setGroup(group).setGroupSummary(summary)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                builder.setGroupAlertBehavior(Notification.GROUP_ALERT_SUMMARY)
            }
        }
        if (summary) {
            val lines = n.optJSONArray("lines")
            val style = Notification.InboxStyle().setBigContentTitle(title)
            if (lines != null) for (i in 0 until lines.length()) style.addLine(lines.getString(i))
            if (!n.isNull("summary_text")) style.setSummaryText(n.optString("summary_text"))
            builder.setStyle(style)
        } else if (!n.isNull("big_text")) {
            builder.setStyle(Notification.BigTextStyle().setBigContentTitle(title).bigText(n.getString("big_text")))
        }
        val target = n.optString("mark_read")
        if (target.isNotEmpty()) {
            val label = if (summary && n.optInt("count") > 1) "Mark all read" else "Mark read"
            val pending = MailActionReceiver.markReadIntent(context, tag, target)
            @Suppress("DEPRECATION")
            builder.addAction(Notification.Action.Builder(R.drawable.ic_launcher_monochrome, label, pending).build())
        }
        if (!n.optBoolean("alert")) builder.muted()
        return builder.build()
    }

    // What the lock screen shows: the account and a count, no sender or
    // subject.
    private fun publicVersion(context: Context, n: JSONObject): Notification =
        builder(context)
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle(n.optString("redacted"))
            .setSubText(n.optString("account").ifEmpty { null })
            .setCategory(Notification.CATEGORY_EMAIL)
            .build()

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

    // One PendingIntent per notification: intents that differ only in
    // extras are the same PendingIntent, so the tag goes into the data URI.
    private fun openIntent(context: Context, tag: String, payload: String): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .setAction(ACTION_OPEN)
            .setData(Uri.fromParts("mailclient", tag, null))
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
