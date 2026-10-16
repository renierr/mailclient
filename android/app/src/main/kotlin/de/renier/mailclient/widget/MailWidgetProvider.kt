package de.renier.mailclient.widget

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.widget.RemoteViews
import de.renier.mailclient.MainActivity
import de.renier.mailclient.R

class MailWidgetProvider : AppWidgetProvider() {

    override fun onUpdate(context: Context, appWidgetManager: AppWidgetManager, appWidgetIds: IntArray) {
        for (id in appWidgetIds) updateWidget(context, appWidgetManager, id)
    }

    companion object {
        const val EXTRA_ACCOUNT = "widget_account"
        const val EXTRA_FOLDER = "widget_folder"
        const val EXTRA_UID = "widget_uid"

        fun updateAll(context: Context) {
            val mgr = AppWidgetManager.getInstance(context)
            val ids = mgr.getAppWidgetIds(ComponentName(context, MailWidgetProvider::class.java))
            if (ids.isEmpty()) return
            for (id in ids) updateWidget(context, mgr, id)
        }

        private fun updateWidget(context: Context, mgr: AppWidgetManager, id: Int) {
            val state = WidgetData.load(context)
            val options = mgr.getAppWidgetOptions(id)
            val minW = options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 0)
            val useList = minW >= 200
            val layout = if (useList) R.layout.widget_list else R.layout.widget_compact
            val rv = RemoteViews(context.packageName, layout)

            val unreadText = if (state.unreadTotal == 1) "1 unread" else "${state.unreadTotal} unread"
            rv.setTextViewText(R.id.widget_unread, unreadText)

            if (useList) {
                val adapterIntent = Intent(context, MailWidgetService::class.java).apply {
                    putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, id)
                    data = android.net.Uri.parse(toUri(Intent.URI_INTENT_SCHEME))
                }
                rv.setRemoteAdapter(id, R.id.widget_list, adapterIntent)
                rv.setEmptyView(R.id.widget_list, R.id.widget_empty)

                // Item click template: opens the tapped message.
                val itemIntent = Intent(context, MailWidgetProvider::class.java).apply {
                    action = ACTION_OPEN_MESSAGE
                }
                val itemPi = PendingIntent.getBroadcast(
                    context, 0, itemIntent,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
                )
                rv.setPendingIntentTemplate(R.id.widget_list, itemPi)

                // Header tap → inbox.
                val headerIntent = Intent(context, MainActivity::class.java).apply {
                    flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                }
                val headerPi = PendingIntent.getActivity(
                    context, 1, headerIntent,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                )
                rv.setOnClickPendingIntent(R.id.widget_root, headerPi)
            } else {
                // Compact: tap → inbox.
                val intent = Intent(context, MainActivity::class.java).apply {
                    flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                }
                val pi = PendingIntent.getActivity(
                    context, 2, intent,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                )
                rv.setOnClickPendingIntent(R.id.widget_root, pi)
            }

            mgr.updateAppWidget(id, rv)
            if (useList) mgr.notifyAppWidgetViewDataChanged(intArrayOf(id), R.id.widget_list)
        }

        const val ACTION_OPEN_MESSAGE = "de.renier.mailclient.widget.OPEN_MESSAGE"

        fun handleOpen(context: Context, intent: Intent) {
            val accountId = intent.getLongExtra(EXTRA_ACCOUNT, -1)
            val folderId = intent.getLongExtra(EXTRA_FOLDER, -1)
            val uid = intent.getIntExtra(EXTRA_UID, -1)
            // Deep link into the app's pending-open mechanism: reuse
            // MainActivity with extras; the shell already handles account/
            // folder jumps. Keep minimal: just open the app.
            val i = Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP
                putExtra("open_account", accountId)
                putExtra("open_folder", folderId)
                putExtra("open_uid", uid)
            }
            context.startActivity(i)
        }
    }

    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action == ACTION_OPEN_MESSAGE) {
            handleOpen(context, intent)
            return
        }
        super.onReceive(context, intent)
    }
}
