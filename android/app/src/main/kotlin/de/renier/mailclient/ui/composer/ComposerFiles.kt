package de.renier.mailclient.ui.composer

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import java.io.File
import java.util.UUID

/** A file picked for sending: the core reads [path] at send time. */
data class PickedFile(val path: String, val name: String)

/**
 * The system picker hands out `content://` URIs; the core sends from file
 * paths (as Flutter's file_picker copies too). Each pick lands in its own
 * app-cache folder under its display name, so the attachment keeps the name
 * the user saw. Blocking: call off the main thread.
 */
object ComposerFiles {
    fun copyIn(context: Context, uri: Uri): PickedFile? = runCatching {
        val name = displayName(context, uri)
        val dir = File(context.cacheDir, "outgoing/${UUID.randomUUID()}").apply { mkdirs() }
        val file = File(dir, name)
        context.contentResolver.openInputStream(uri)?.use { input ->
            file.outputStream().use { input.copyTo(it) }
        } ?: return null
        PickedFile(file.absolutePath, name)
    }.getOrNull()

    private fun displayName(context: Context, uri: Uri): String {
        val queried = runCatching {
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
        }.getOrNull()
        // No path separators in a name we create a file from.
        val name = (queried ?: uri.lastPathSegment ?: "attachment").substringAfterLast('/').trim()
        return name.ifEmpty { "attachment" }
    }
}
