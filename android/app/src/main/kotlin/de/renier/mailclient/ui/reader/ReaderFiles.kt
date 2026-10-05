package de.renier.mailclient.ui.reader

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.activity.result.contract.ActivityResultContract
import androidx.core.content.FileProvider
import de.renier.mailclient.MailNative
import java.io.File

// Attachment and export file plumbing for the reader: the core caches and
// names the bytes; this side only hands them to Android (FileProvider for
// viewers, the Storage Access Framework for saving). Blocking — callers run
// it off the main thread.
object ReaderFiles {
    /** Cached bytes, fetched from the account's server first when missing. */
    fun ensureBytes(accountId: Long, folderId: Long, uid: Int, attachmentId: Long): ByteArray =
        try {
            MailNative.cachedAttachmentBytes(attachmentId)
        } catch (_: Exception) {
            MailNative.downloadMessageFiles(accountId, folderId, uid)
            MailNative.cachedAttachmentBytes(attachmentId)
        }

    /** A viewer intent for one attachment, via a cache-private copy. */
    fun openIntent(
        context: Context,
        accountId: Long,
        folderId: Long,
        uid: Int,
        attachmentId: Long,
        mime: String,
    ): Intent {
        ensureBytes(accountId, folderId, uid, attachmentId)
        val dir = File(context.cacheDir, "mailclient-attachments").apply { mkdirs() }
        val path = MailNative.writeAttachmentCopy(attachmentId, dir.path)
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.readerfiles", File(path))
        val view = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, mime)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        return Intent.createChooser(view, "Open with")
    }

    fun write(context: Context, uri: Uri, bytes: ByteArray) {
        context.contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
            ?: throw IllegalStateException("cannot write")
    }

    /** Write each file into a picked directory tree; the count written. */
    fun writeAll(context: Context, tree: Uri, files: List<SaveFile>): Int {
        val parent = DocumentsContract.buildDocumentUriUsingTree(
            tree,
            DocumentsContract.getTreeDocumentId(tree),
        )
        var n = 0
        for (f in files) {
            val doc = DocumentsContract.createDocument(context.contentResolver, parent, f.mime, f.name) ?: continue
            runCatching {
                context.contentResolver.openOutputStream(doc)?.use { it.write(f.bytes) }
                n++
            }
        }
        return n
    }
}

class SaveFile(val name: String, val mime: String, val bytes: ByteArray)

/** CREATE_DOCUMENT with the type chosen per launch: (suggested name, MIME). */
class CreateTypedDocument : ActivityResultContract<Pair<String, String>, Uri?>() {
    override fun createIntent(context: Context, input: Pair<String, String>): Intent =
        Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = input.second
            putExtra(Intent.EXTRA_TITLE, input.first)
        }

    override fun parseResult(resultCode: Int, intent: Intent?): Uri? =
        if (resultCode == android.app.Activity.RESULT_OK) intent?.data else null
}
