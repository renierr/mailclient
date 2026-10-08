package de.renier.mailclient.ui.common

import org.json.JSONArray
import org.json.JSONObject

// Small readers for the core's JSON payloads; a missing array or object
// reads as empty.

/** Every entry as a string. */
fun JSONArray?.strings(): List<String> = if (this == null) emptyList() else List(length()) { optString(it) }

/** The entries that are objects, in order. */
fun JSONArray?.objects(): List<JSONObject> =
    if (this == null) emptyList() else (0 until length()).mapNotNull { optJSONObject(it) }

/** Every field as a string. */
fun JSONObject?.stringMap(): Map<String, String> =
    if (this == null) emptyMap() else keys().asSequence().associateWith { optString(it) }
