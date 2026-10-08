package de.renier.mailclient.ui.reader

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.core.content.FileProvider
import de.renier.mailclient.MailNative
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.isAlreadyRunning
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

// Attachment and export file plumbing for the reader: the core caches and
// names the bytes; this side only hands them to Android (FileProvider for
// viewers, the Storage Access Framework for saving).
object ReaderFiles {
    private const val DOWNLOAD_WAIT_MS = 120_000L

    // Finish events of the shared `Attachments` kind to sit through before
    // giving up on one message's download.
    private const val MAX_FINISH_WAITS = 5

    /** A download the server or network refused; [message] is the job's error. */
    class DownloadFailed(message: String) : Exception(message)

    /**
     * Cached bytes, downloading first when the message arrived without them.
     *
     * The download is the queued `Attachments` job — awaited like Flutter's
     * attachmentBytes: the waiter is registered before queueing so a fast
     * download cannot slip through, and a tap while one runs ("already
     * running") waits for its event instead of stacking. Null when the bytes
     * never landed. Suspends on the caller's thread; the job itself runs on
     * the core's net thread, never a second IMAP session off it.
     */
    suspend fun ensureBytes(
        state: MailState,
        accountId: Long,
        folderId: Long,
        uid: Int,
        attachmentId: Long,
    ): ByteArray? {
        runCatching { MailNative.cachedAttachmentBytes(attachmentId) }.getOrNull()?.let { return it }
        withContext(Dispatchers.Main) { state.info("Downloading attachment…") }
        // Parallel downloads share the `Attachments` kind, so a finish event
        // may belong to another message's job: keep waiting while nothing
        // arrived.
        repeat(MAX_FINISH_WAITS) {
            val done = try {
                state.awaitFinished("Attachments", DOWNLOAD_WAIT_MS) {
                    MailNative.downloadAttachments(accountId, folderId, uid)
                }
            } catch (e: Exception) {
                if (!e.isAlreadyRunning()) {
                    // The job never started; there is no finish event coming.
                    return runCatching { MailNative.cachedAttachmentBytes(attachmentId) }.getOrNull()
                }
                // Otherwise the bytes are on their way already — wait below.
                state.awaitFinished("Attachments", DOWNLOAD_WAIT_MS) {}
            }
            val bytes = runCatching { MailNative.cachedAttachmentBytes(attachmentId) }.getOrNull()
            if (bytes != null) return bytes
            if (done != null && !done.first) throw DownloadFailed(done.second.ifEmpty { "Download failed" })
        }
        return null
    }

    /**
     * Queue a whole-message download and wait until it has finished (the
     * inline-images banner, a forward, a reopened draft: no single
     * attachment id to re-read). Downloads of other messages share the
     * `Attachments` event, so each finish asks the core whether this one is
     * still running. True unless the wait timed out or the download could
     * not start; the caller re-reads either way.
     */
    suspend fun downloadAll(state: MailState, accountId: Long, folderId: Long, uid: Int): Boolean {
        withContext(Dispatchers.Main) { state.info("Downloading…") }
        var queue: () -> Unit = { MailNative.downloadAttachments(accountId, folderId, uid) }
        repeat(MAX_FINISH_WAITS) {
            val done = try {
                state.awaitFinished("Attachments", DOWNLOAD_WAIT_MS, queue)
            } catch (e: Exception) {
                if (!e.isAlreadyRunning()) return false
                // Ours is on its way already: wait for finishes below.
                state.awaitFinished("Attachments", DOWNLOAD_WAIT_MS) {}
            }
            queue = {}
            if (done == null) return false
            if (!runCatching { MailNative.attachmentsPending(folderId, uid) }.getOrDefault(false)) return true
        }
        return true
    }

    /** A viewer intent for one attachment, via a cache-private copy. */
    suspend fun openIntent(
        state: MailState,
        context: Context,
        accountId: Long,
        folderId: Long,
        uid: Int,
        attachmentId: Long,
        mime: String,
    ): Intent {
        ensureBytes(state, accountId, folderId, uid, attachmentId)
            ?: throw IllegalStateException("attachment is not downloaded yet")
        // Fresh from the stored row, not the reader payload's copy: the
        // download-time magic check may have corrected the header since
        // the message was read, and the old MIME would pick the wrong app.
        val opener = runCatching { MailNative.attachmentOpenMime(attachmentId) }
            .getOrDefault(mime)
        val dir = File(context.cacheDir, "mailclient-attachments").apply { mkdirs() }
        val path = MailNative.writeAttachmentCopy(attachmentId, dir.path)
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.readerfiles", File(path))
        val view = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, opener)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        return Intent.createChooser(view, "Open with")
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
