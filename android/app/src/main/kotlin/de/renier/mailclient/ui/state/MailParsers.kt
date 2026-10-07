package de.renier.mailclient.ui.state

import org.json.JSONObject

// Feed JSON decoders for MailState. Pure functions over core feed fields
// (avatar letters/colours, depth/leaf, delete rule all decided by mailcore);
// the frontends only carry the result. Previously `MailState`'s companion;
// moved here verbatim — no caller used the `MailState.` qualifier.

internal fun parseAccounts(json: String): List<Account> {
    val arr = runCatching { org.json.JSONArray(json) }.getOrElse { return emptyList() }
    return List(arr.length()) { i ->
        val o = arr.optJSONObject(i) ?: JSONObject()
        Account(
            id = o.optLong("id", -1),
            email = o.optString("email"),
            name = o.optString("name").ifEmpty { o.optString("email") },
            fromName = o.optString("from_name").takeIf { it != "null" }.orEmpty(),
            initials = o.optString("initials", "?"),
            avatarLight = o.optString("avatar_light"),
            avatarDark = o.optString("avatar_dark"),
        )
    }.filter { it.id >= 0 }
}

internal fun parseFolders(json: String): List<Folder> {
    val arr = runCatching { org.json.JSONArray(json) }.getOrElse { return emptyList() }
    return List(arr.length()) { i ->
        val o = arr.optJSONObject(i) ?: JSONObject()
        val path = o.optString("name")
        val role = o.optString("role")
        Folder(
            id = o.optLong("id", -1),
            path = path,
            leaf = o.optString("leaf").ifEmpty { path },
            depth = o.optInt("depth", 0),
            role = role,
            unread = o.optInt("unread", 0),
            count = o.optInt("count", 0),
            subscribed = o.optBoolean("subscribed", true),
            alwaysVisible = o.optBoolean("always_visible", role != "custom"),
            deleteIsPermanent = o.optBoolean("delete_is_permanent", false),
        )
    }.filter { it.id >= 0 }
}

internal fun parseMessages(json: String): List<MessageRow> {
    val arr = runCatching { org.json.JSONArray(json) }.getOrElse { return emptyList() }
    return List(arr.length()) { i ->
        val o = arr.optJSONObject(i) ?: JSONObject()
        MessageRow(
            uid = o.optInt("uid", -1),
            subject = o.optString("subject", "(no subject)"),
            from = o.optString("from"),
            fromName = o.optString("from_name"),
            date = o.optString("date"),
            snippet = o.optString("snippet"),
            unread = o.optBoolean("unread", false),
            starred = o.optBoolean("starred", false),
            hasAttachments = o.optBoolean("has_attachments", false),
            dateRaw = o.optString("date_raw"),
            initials = o.optString("initials", "?"),
            avatarLight = o.optString("avatar_light"),
            avatarDark = o.optString("avatar_dark"),
            folderId = o.optLong("folder_id", -1),
        )
    }.filter { it.uid >= 0 }
}
