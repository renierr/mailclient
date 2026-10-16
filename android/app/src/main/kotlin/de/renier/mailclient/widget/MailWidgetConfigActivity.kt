package de.renier.mailclient.widget

import android.app.Activity
import android.appwidget.AppWidgetManager
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.widget.Toast
import de.renier.mailclient.MailNative
import org.json.JSONArray

class MailWidgetConfigActivity : Activity() {
    private var widgetId: Int = AppWidgetManager.INVALID_APPWIDGET_ID

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        widgetId = intent.getIntExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, AppWidgetManager.INVALID_APPWIDGET_ID)
        if (widgetId == AppWidgetManager.INVALID_APPWIDGET_ID) {
            finish()
            return
        }
        MailNative.ensureInit(applicationContext)

        val accounts = runCatching { JSONArray(MailNative.accountsJson()) }.getOrDefault(JSONArray())
        if (accounts.length() == 0) {
            Toast.makeText(this, "Add an account first", Toast.LENGTH_SHORT).show()
            finish()
            return
        }
        val labels = Array(accounts.length()) { i ->
            val o = accounts.optJSONObject(i)
            val name = o?.optString("name").orEmpty()
            val email = o?.optString("email").orEmpty()
            if (name.isNotBlank()) "$name  <$email>" else email
        }
        val ids = LongArray(accounts.length()) { i -> accounts.optJSONObject(i)?.optLong("id", -1) ?: -1 }

        android.app.AlertDialog.Builder(this)
            .setTitle("Widget inbox")
            .setItems(labels) { _, which ->
                val acctId = ids[which]
                getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                    .edit().putLong(key(widgetId), acctId).apply()
                MailWidgetProvider.updateWidget(this, AppWidgetManager.getInstance(this), widgetId)
                val result = Intent().putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, widgetId)
                setResult(RESULT_OK, result)
                finish()
            }
            .setOnCancelListener { finish() }
            .show()
    }

    companion object {
        const val PREFS = "mail_widget"
        fun key(widgetId: Int) = "account_$widgetId"

        fun selectedAccount(context: Context, widgetId: Int): Long {
            return context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
                .getLong(key(widgetId), -1)
        }
    }
}
