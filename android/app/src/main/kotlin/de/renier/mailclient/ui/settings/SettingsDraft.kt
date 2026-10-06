package de.renier.mailclient.ui.settings

import androidx.compose.runtime.mutableStateMapOf
import org.json.JSONArray
import org.json.JSONObject

/** A settings value in its stored string form: flags `"1"`/`"0"`. */
fun rawOf(v: Any?): String = when (v) {
    null, JSONObject.NULL -> ""
    is Boolean -> if (v) "1" else "0"
    else -> v.toString()
}

/**
 * The form's working copy of every preference, as stored strings, from the
 * core's `settingsJson`. Nothing is written until Save, and then only what
 * changed ([changes]); the sort pair goes through its own call.
 */
class SettingsDraft(saved: JSONObject, choicesJson: JSONObject) {
    private val saved: Map<String, String> = saved.keys().asSequence().associateWith { rawOf(saved.opt(it)) }
    private val values = mutableStateMapOf<String, String>().apply { putAll(this@SettingsDraft.saved) }

    // Offered values per pick-one key, in display order, as stored strings.
    private val offered: Map<String, List<String>> = choicesJson.keys().asSequence().associateWith { key ->
        val arr = choicesJson.optJSONObject(key)?.optJSONArray("values") ?: JSONArray()
        (0 until arr.length()).map { rawOf(arr.opt(it)) }
    }

    operator fun get(key: String): String = values[key].orEmpty()

    operator fun set(key: String, value: String) {
        values[key] = value
    }

    fun flag(key: String): Boolean = values[key] == "1"

    fun setFlag(key: String, on: Boolean) = set(key, if (on) "1" else "0")

    fun options(key: String): List<String> = offered[key].orEmpty()

    /** Changed keys and their new values, the sort pair left out. */
    fun changes(): Map<String, String> = values.filter { (k, v) -> k !in SORT_KEYS && saved[k] != v }

    val sortChanged: Boolean get() = SORT_KEYS.any { saved[it] != values[it] }

    val dirty: Boolean get() = values.any { (k, v) -> saved[k] != v }

    companion object {
        val SORT_KEYS = setOf("message_sort_field", "message_sort_desc")
    }
}

/**
 * One account's overrides being edited: key → stored string, `""` (or
 * absent) for "use the app-wide value". [effectiveHeartbeat] is the
 * server's frequent IDLE heartbeat gap when there is one.
 */
class AccountDraft(view: JSONObject) {
    private val saved: Map<String, String> = view.optJSONObject("overrides")?.let { o ->
        o.keys().asSequence().associateWith { o.optString(it) }
    }.orEmpty()
    private val values = mutableStateMapOf<String, String>().apply { putAll(this@AccountDraft.saved) }
    val heartbeatSecs: Long? = if (view.isNull("frequent_heartbeat_secs")) null else view.optLong("frequent_heartbeat_secs")

    operator fun get(key: String): String = values[key].orEmpty()

    operator fun set(key: String, value: String) {
        values[key] = value
    }

    /** Every key whose override changed, `""` meaning "inherit again". */
    fun changes(): Map<String, String> =
        (saved.keys + values.keys).filter { (saved[it] ?: "") != (values[it] ?: "") }
            .associateWith { values[it] ?: "" }

    val dirty: Boolean get() = changes().isNotEmpty()
}
