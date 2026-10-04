package de.renier.mailclient

import android.content.Context

// The Rust core (libmailffi.so, crates/mailffi/src/android.rs) for the parts
// that run without a Flutter engine: the check worker, the exact alarm, the
// push service and the notification buttons. Same library and database as
// the Dart side, loaded once per process.
object MailNative {
    @Volatile private var ready = false

    init {
        System.loadLibrary("mailffi")
    }

    // Point the core at the app's files dir (the same directory Dart's
    // getApplicationSupportDirectory() returns) and open the database.
    fun ensureInit(context: Context) {
        if (ready) return
        synchronized(this) {
            if (!ready) {
                init(context.applicationContext.filesDir.path)
                ready = true
            }
        }
    }

    @JvmStatic private external fun init(dataDir: String)

    // What to run in the background now (BackgroundPlan JSON): push,
    // poll_minutes, poll_scheduler, quiet_accounts, replan_at. No network.
    @JvmStatic external fun backgroundPlan(): String

    // One scheduled check; blocks for the network run. BackgroundReport JSON.
    @JvmStatic external fun check(trigger: String): String

    // NotificationPlan JSON for a report. `shown`: JSON object of the mail
    // notifications on screen, tag -> signature.
    @JvmStatic external fun plan(report: String, permitted: Boolean, foreground: Boolean, shown: String): String

    // A "Mark read" button: mark the ReadTarget JSON read in the cache, no
    // network. BackgroundReport JSON of what is still pending, for plan().
    @JvmStatic external fun markRead(target: String): String

    // Send an account's queued flag changes; blocks for the network, throws
    // when the server cannot be reached.
    @JvmStatic external fun pushFlags(accountId: Long)

    // The plan was carried out: commit its marks and outcome.
    @JvmStatic external fun commit(plan: String)

    @JvmStatic external fun recordOutcome(run: String, outcome: String)

    @JvmStatic external fun pushStart(callbacks: PushCallbacks, online: Boolean)

    @JvmStatic external fun pushKeepalive()

    @JvmStatic external fun pushNetwork(online: Boolean)

    @JvmStatic external fun pushStop()

    // Experiment: native reader (branch `experiment/native-reader`).
    // ReaderActivity passes ids only and re-reads from the same database
    // Dart uses — bodies with inline images exceed the Binder limit.
    // Full reader payload, same JSON as mailffi `message_json`.
    @JvmStatic external fun readerMessage(folderId: Long, uid: Int): String

    // `{from, to, cc, date, subject, message_id, reply_to}` headers view.
    @JvmStatic external fun readerHeaders(folderId: Long, uid: Int): String

    // Full WebView document for a sanitized body. `paint` is
    // `theme`/`original`/`darkened`; colours are `0xRRGGBB`; `top_space` is
    // always 0 (the header is native views above the WebView, no overlay).
    @JvmStatic external fun readerDocument(
        body: String,
        paint: String,
        paper: Int,
        ink: Int,
        link: Int,
        quote: Int,
        rule: Int,
        allowRemote: Boolean,
        scale: Float,
        fit: Boolean,
    ): String

    // Re-sanitized HTML with remote images kept — the "show once" path.
    @JvmStatic external fun readerMessageHtml(folderId: Long, uid: Int, allowRemote: Boolean): String

    // Whether and when opening an unread row marks it read:
    // `{"plan":"off"|"now"|"after","delay_secs":n}`.
    @JvmStatic external fun markReadPlan(autoMarkRead: Boolean, delaySecs: Long, unread: Boolean): String

    // Local flag write with background push (like `mark_read`).
    @JvmStatic external fun setReadFlag(accountId: Long, folderId: Long, uid: Int, read: Boolean)

    // Flip one message's starred flag; re-read the message for the state.
    @JvmStatic external fun toggleStar(accountId: Long, folderId: Long, uid: Int)

    // Undoable queue results: `{"batch","label","purging"}`.
    @JvmStatic external fun deleteMessage(accountId: Long, folderId: Long, uid: Int): String
    @JvmStatic external fun archiveMessage(accountId: Long, folderId: Long, uid: Int): String
    @JvmStatic external fun moveMessage(accountId: Long, folderId: Long, uid: Int, destPath: String): String

    // Destroy server-side. No undo — the UI confirms first.
    @JvmStatic external fun purgeMessage(accountId: Long, folderId: Long, uid: Int)

    // Take back a queued action; the status line text, also when too late.
    @JvmStatic external fun undoMove(batch: String): String

    // Seconds an action stays undoable.
    @JvmStatic external fun undoGraceSecs(): String

    // The move picker's folder tree.
    @JvmStatic external fun foldersJson(accountId: Long): String

    // A clicked link split for the examine dialog:
    // `{"safe","scheme","host","path"}`.
    @JvmStatic external fun linkInfo(url: String): String

    // One cached attachment's bytes; throws when not downloaded yet.
    @JvmStatic external fun cachedAttachmentBytes(attachmentId: Long): ByteArray

    // Fetch every attachment of one message (blocking); how many landed.
    @JvmStatic external fun downloadMessageFiles(accountId: Long, folderId: Long, uid: Int): String

    // The viewer copy of a cached attachment, under a safe name in `dir`.
    @JvmStatic external fun writeAttachmentCopy(attachmentId: Long, dir: String): String

    // Filesystem-safe `.eml` name for one message.
    @JvmStatic external fun suggestedEmlName(folderId: Long, uid: Int): String

    // Download-then-assemble as one blocking call (Dart waits for a job).
    @JvmStatic external fun exportEmlBytes(folderId: Long, uid: Int): ByteArray
}

// What the push monitor calls back on its own thread (MailNative.pushStart).
interface PushCallbacks {
    // Work started (true) or everything is waiting again (false).
    fun onBusy(busy: Boolean)

    // BackgroundReport JSON of a finished check.
    fun onReport(report: String)
}
