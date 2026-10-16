package de.renier.mailclient.widget

import android.content.Intent
import android.widget.RemoteViewsService

class MailWidgetService : RemoteViewsService() {
    override fun onGetViewFactory(intent: Intent): RemoteViewsService.RemoteViewsFactory =
        MailWidgetFactory(applicationContext)
}
