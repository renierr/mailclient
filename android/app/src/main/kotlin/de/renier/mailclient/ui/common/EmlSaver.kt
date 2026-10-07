package de.renier.mailclient.ui.common

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContract
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** CREATE_DOCUMENT with the type chosen per launch: (suggested name, MIME). */
class CreateTypedDocument : ActivityResultContract<Pair<String, String>, Uri?>() {
    override fun createIntent(context: Context, input: Pair<String, String>): Intent =
        Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = input.second
            putExtra(Intent.EXTRA_TITLE, input.first)
        }

    override fun parseResult(resultCode: Int, intent: Intent?): Uri? =
        if (resultCode == Activity.RESULT_OK) intent?.data else null
}

/** Write bytes to a SAF Uri; throws when the stream cannot be opened. */
fun writeBytes(context: Context, uri: Uri, bytes: ByteArray) {
    context.contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
        ?: throw IllegalStateException("cannot write")
}

/**
 * Save one RFC822 message through the SAF picker: export the bytes and the
 * core's suggested file name, then write the picked document. The returned
 * lambda takes the message's folder id and uid. `onSaved` / `onFailed` run
 * on the main thread for status lines.
 */
@Composable
fun rememberEmlSaver(
    onSaved: () -> Unit,
    onFailed: (String) -> Unit,
): (folderId: Long, uid: Int) -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var pending by remember { mutableStateOf<ByteArray?>(null) }
    val picker = rememberLauncherForActivityResult(CreateTypedDocument()) { uri ->
        val bytes = pending
        pending = null
        if (uri != null && bytes != null) {
            scope.launch(Dispatchers.IO) {
                val ok = runCatching { writeBytes(context, uri, bytes) }.isSuccess
                withContext(Dispatchers.Main) {
                    if (ok) onSaved() else onFailed("Could not save the message")
                }
            }
        }
    }
    return { folderId, uid ->
        scope.launch(Dispatchers.IO) {
            val exported = runCatching {
                MailNative.exportEmlBytes(folderId, uid) to MailNative.suggestedEmlName(folderId, uid)
            }
            val (bytes, name) = exported.getOrElse {
                withContext(Dispatchers.Main) { onFailed(it.message ?: "Could not export the message") }
                return@launch
            }
            withContext(Dispatchers.Main) {
                pending = bytes
                picker.launch(name to "message/rfc822")
            }
        }
    }
}
