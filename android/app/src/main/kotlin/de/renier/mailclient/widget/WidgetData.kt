package de.renier.mailclient.widget

import android.content.Context
import de.renier.mailclient.MailNative
import org.json.JSONArray
import org.json.JSONObject

data class WidgetMessage(
    val uid: Int,
    val folderId: Long,
    val accountId: Long,
    val sender: String,
    val subject: String,
    val time: String,
    val unread: Boolean,
    val initials: String,
    val avatarColor: Int,
)

data class WidgetState(
    val accountId: Long,
    val folderId: Long,
    val unreadTotal: Int,
    val messages: List<WidgetMessage>,
    val offline: Boolean,
)

object WidgetData {
    fun load(context: Context): WidgetState {
        MailNative.ensureInit(context)
        val accounts = runCatching { JSONArray(MailNative.accountsJson()) }.getOrDefault(JSONArray())
        if (accounts.length() == 0) {
            return WidgetState(-1, -1, 0, emptyList(), false)
        }
        // Default to the first account's inbox.
        val acct = accounts.optJSONObject(0) ?: return WidgetState(-1, -1, 0, emptyList(), false)
        val accountId = acct.optLong("id", -1)
        if (accountId < 0) return WidgetState(-1, -1, 0, emptyList(), false)

        val folders = runCatching { JSONArray(MailNative.foldersJson(accountId)) }.getOrDefault(JSONArray())
        var inboxId = -1L
        var unread = 0
        for (i in 0 until folders.length()) {
            val f = folders.optJSONObject(i) ?: continue
            if (f.optString("role") == "inbox") {
                inboxId = f.optLong("id", -1)
                unread = f.optInt("unread", 0)
                break
            }
        }
        if (inboxId < 0 && folders.length() > 0) {
            val f = folders.optJSONObject(0)
            inboxId = f?.optLong("id", -1) ?: -1
            unread = f?.optInt("unread", 0) ?: 0
        }

        val msgs = mutableListOf<WidgetMessage>()
        if (inboxId >= 0) {
            val arr = runCatching { JSONArray(MailNative.messagesJson(inboxId, 5, 0)) }.getOrDefault(JSONArray())
            for (i in 0 until arr.length()) {
                val o = arr.optJSONObject(i) ?: continue
                val uid = o.optInt("uid", -1)
                if (uid < 0) continue
                val fromName = o.optString("from_name")
                val from = o.optString("from")
                val sender = if (fromName.isNotBlank()) fromName else from
                val colorStr = o.optString("avatar_light", "")
                val color = runCatching { android.graphics.Color.parseColor(colorStr) }.getOrDefault(0xFF3B82F6.toInt())
                msgs += WidgetMessage(
                    uid = uid,
                    folderId = inboxId,
                    accountId = accountId,
                    sender = sender,
                    subject = o.optString("subject", "(no subject)"),
                    time = o.optString("date"),
                    unread = o.optBoolean("unread", false),
                    initials = o.optString("initials", "?"),
                    avatarColor = color,
                )
            }
        }
        return WidgetState(accountId, inboxId, unread, msgs, false)
    }
}
