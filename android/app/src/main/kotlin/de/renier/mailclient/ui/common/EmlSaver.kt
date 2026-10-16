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
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.isAlreadyRunning
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

private const val EXPORT_WAIT_MS = 120_000L

/**
 * Save one RFC822 message through the SAF picker. Missing files download in
 * the core's queued "Export" job (status line and busy state like any
 * job, the same as Qt); once it finishes the bytes and the core's suggested
 * file name are read locally and the picked document is written. The
 * returned lambda takes the message's folder id and uid. `onSaved` /
 * `onFailed` run on the main thread for status lines.
 */
@Composable
fun rememberEmlSaver(
    state: MailState,
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
        scope.launch {
            val key = "export:$folderId:$uid"
            val queue = {
                MailNative.ensureInit(context)
                MailNative.exportFetch(folderId, uid)
            }
            val done = try {
                state.awaitFinished(key, EXPORT_WAIT_MS, queue)
            } catch (e: Exception) {
                if (!e.isAlreadyRunning()) {
                    onFailed(e.message ?: "Could not export the message")
                    return@launch
                }
                // A tap while ours runs: wait for that job's finish instead.
                state.awaitFinished(key, EXPORT_WAIT_MS) {}
            }
            if (done != null && !done.first) {
                onFailed(done.second.ifEmpty { "Could not export the message" })
                return@launch
            }
            // A timed-out wait still tries: the assembly refuses while
            // files are missing.
            val exported = withContext(Dispatchers.IO) {
                runCatching {
                    MailNative.assembleEml(folderId, uid) to MailNative.suggestedEmlName(folderId, uid)
                }
            }
            val (bytes, name) = exported.getOrElse {
                onFailed(it.message ?: "Could not export the message")
                return@launch
            }
            pending = bytes
            picker.launch(name to "message/rfc822")
        }
    }
}
