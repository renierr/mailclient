package de.renier.mailclient.widget

import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.widget.RemoteViews
import android.widget.RemoteViewsService
import de.renier.mailclient.R

class MailWidgetFactory(private val context: Context) : RemoteViewsService.RemoteViewsFactory {
    private var state: WidgetState = WidgetState(-1, -1, 0, emptyList(), false)

    override fun onCreate() {
        state = WidgetData.load(context)
    }

    override fun onDataSetChanged() {
        state = WidgetData.load(context)
    }

    override fun onDestroy() {}

    override fun getCount(): Int = state.messages.size

    override fun getViewAt(position: Int): RemoteViews {
        val rv = RemoteViews(context.packageName, R.layout.widget_row)
        val msg = state.messages.getOrElse(position) { return rv }
        rv.setTextViewText(R.id.row_sender, msg.sender)
        rv.setTextViewText(R.id.row_subject, msg.subject)
        rv.setTextViewText(R.id.row_time, msg.time)
        rv.setTextViewText(R.id.row_avatar, msg.initials)
        // Tint avatar background via a color filter on the drawable is not
        // possible on RemoteViews easily; set text color only.
        rv.setTextColor(R.id.row_avatar, Color.WHITE)
        // Fill-in intent carries uid/folder/account to the provider's
        // pending-intent template.
        val fillIn = Intent().apply {
            putExtra(MailWidgetProvider.EXTRA_ACCOUNT, msg.accountId)
            putExtra(MailWidgetProvider.EXTRA_FOLDER, msg.folderId)
            putExtra(MailWidgetProvider.EXTRA_UID, msg.uid)
        }
        rv.setOnClickFillInIntent(R.id.widget_row_root, fillIn)
        return rv
    }

    override fun getLoadingView(): RemoteViews? = null

    override fun getViewTypeCount(): Int = 1

    override fun getItemId(position: Int): Long = state.messages.getOrElse(position) { null }?.uid?.toLong() ?: position.toLong()

    override fun hasStableIds(): Boolean = true
}
