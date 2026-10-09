package de.renier.mailclient

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.provider.ContactsContract
import androidx.core.content.ContextCompat
import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/**
 * The phone's own contact list for the composer's recipient field. Read
 * once per process (READ_CONTACTS) and handed to the core, which merges it
 * with the mail-collected contacts and ranks a saved person first
 * (`mailcore::store::contacts`). Nothing is stored here: the snapshot lives
 * in the core process while the feature is on and is dropped by
 * [dropSnapshot] when the setting is switched off.
 */
object PhoneContacts {
    /** How many e-mail rows to read at most; merging runs over all of them. */
    private const val LIMIT = 5000

    @Volatile
    private var pushed = false

    fun granted(context: Context): Boolean =
        ContextCompat.checkSelfPermission(context, Manifest.permission.READ_CONTACTS) ==
            PackageManager.PERMISSION_GRANTED

    /** Read the phone book and push it to the core; no read twice. */
    suspend fun loadOnce(context: Context): Boolean {
        if (pushed || !granted(context)) return false
        pushed = true
        return withContext(Dispatchers.IO) {
            runCatching { MailNative.setPhoneContacts(read(context)) > 0 }.getOrDefault(false)
        }
    }

    /** Forget the snapshot is loaded; the next [loadOnce] reads again. */
    fun forget() {
        pushed = false
    }

    /** The setting went off: the core drops the phone book from memory. */
    suspend fun dropSnapshot() {
        forget()
        withContext(Dispatchers.IO) { runCatching { MailNative.clearPhoneContacts() } }
    }

    private fun read(context: Context): String {
        val nameColumn = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            ContactsContract.Contacts.DISPLAY_NAME_PRIMARY
        } else {
            @Suppress("DEPRECATION")
            ContactsContract.Contacts.DISPLAY_NAME
        }
        val rows = JSONArray()
        context.contentResolver.query(
            ContactsContract.CommonDataKinds.Email.CONTENT_URI,
            arrayOf(nameColumn, ContactsContract.CommonDataKinds.Email.ADDRESS),
            null,
            null,
            "$nameColumn LIMIT $LIMIT",
        )?.use { cursor ->
            val nameIndex = cursor.getColumnIndex(nameColumn)
            val addressIndex = cursor.getColumnIndex(ContactsContract.CommonDataKinds.Email.ADDRESS)
            while (cursor.moveToNext()) {
                val address = cursor.getString(addressIndex)?.trim().orEmpty()
                if (!address.contains('@')) continue
                // The core fills a nameless entry from the mail it meets
                // and drops automated senders itself.
                val name = if (nameIndex >= 0) cursor.getString(nameIndex)?.trim().orEmpty() else ""
                rows.put(JSONObject().put("name", name).put("address", address))
            }
        }
        return rows.toString()
    }
}
