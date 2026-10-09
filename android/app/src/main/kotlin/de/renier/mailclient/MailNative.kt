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
    // notifications on screen, tag -> {"title", "body"} as read back; the
    // core builds the signatures it compares.
    @JvmStatic external fun plan(report: String, permitted: Boolean, foreground: Boolean, shown: String): String

    // A "Mark read" button: mark the ReadTarget JSON read in the cache, no
    // network. BackgroundReport JSON of what is still pending, for plan().
    @JvmStatic external fun markRead(target: String): String

    // The account a ReadTarget JSON belongs to; the JSON stays opaque here.
    @JvmStatic external fun readTargetAccount(target: String): Long

    // A notification's Archive or Delete (its plan's quick_action): queued
    // with no undo window, cache only. BackgroundReport JSON for plan();
    // throws when the delete would destroy the mail.
    @JvmStatic external fun notifyAct(target: String, action: String): String

    // A notification's Reply: the core builds and queues the reply, then it
    // is delivered on the network thread. Blocks until sent (worker thread
    // only); returns the status, throws when it was not sent.
    @JvmStatic external fun notifyReply(target: String, text: String): String

    // Send an account's queued flag changes and due moves; blocks for the
    // network, throws when the server cannot be reached.
    @JvmStatic external fun pushChanges(accountId: Long)

    // The plan was carried out: commit its marks and outcome.
    @JvmStatic external fun commit(plan: String)

    @JvmStatic external fun recordOutcome(run: String, outcome: String)

    @JvmStatic external fun pushStart(callbacks: PushCallbacks, online: Boolean)

    @JvmStatic external fun pushKeepalive()

    @JvmStatic external fun pushNetwork(online: Boolean)

    @JvmStatic external fun pushStop()

    // Reader: ids in, everything re-read from the same database the other
    // frontends use. Full reader payload, same JSON as mailffi `message_json`.
    @JvmStatic external fun readerMessage(folderId: Long, uid: Int): String

    // `{from, to, cc, date, subject, message_id, reply_to}` headers view.
    @JvmStatic external fun readerHeaders(folderId: Long, uid: Int): String

    // Still listed (cached, not waiting out an undoable move)? A reader
    // whose message is gone closes.
    @JvmStatic external fun messageListed(folderId: Long, uid: Int): Boolean

    // Back in the foreground: sync, or did one finish moments ago?
    @JvmStatic external fun resumeSyncDue(accountId: Long): Boolean

    // Re-sanitized HTML with remote images kept — the "show once" path.
    @JvmStatic external fun readerMessageHtml(folderId: Long, uid: Int, allowRemote: Boolean): String

    // What an empty message list says (core words for every case).
    @JvmStatic external fun emptyListText(
        searching: Boolean,
        serverSearching: Boolean,
        quickFilter: Boolean,
        unfiltered: Int,
        query: String,
    ): String

    // The reader text size's factor on mail text (core table).
    @JvmStatic external fun readerTextScale(size: String): Float

    // Whether a delete destroys and whether to ask first:
    // `{"permanent","ask"}`. One `delete_is_permanent` (or null) per target.
    @JvmStatic external fun deletePrompt(confirmPref: Boolean, bulk: Boolean, permanentJson: String): String

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

    // Painted sidebar rows (`[{id, collapsible, expanded, unread, total}]`)
    // for the expanded folder ids in expandedJson (`[]` = all collapsed).
    @JvmStatic external fun sidebarRowsJson(accountId: Long, expandedJson: String): String

    // A clicked link split for the examine dialog:
    // `{"safe","scheme","host","path"}`.
    @JvmStatic external fun linkInfo(url: String): String

    // One cached attachment's bytes; throws when not downloaded yet.
    @JvmStatic external fun cachedAttachmentBytes(attachmentId: Long): ByteArray

    // Queue fetching every attachment of one message into the cache; the
    // bytes land with the `Attachments` finished event. Throws when a
    // download is already queued (`spawn` dedupe) — then wait for its
    // event instead of queueing again.
    @JvmStatic external fun downloadAttachments(accountId: Long, folderId: Long, uid: Int)

    // The in-flight keys jobs report under (the "key" of their finish event),
    // so a waiter registers for exactly its own job before queuing it.
    @JvmStatic external fun attachmentsJobKey(folderId: Long, uid: Int): String
    @JvmStatic external fun createFolderJobKey(accountId: Long): String
    // That message's download is still queued or running (the event kind is shared).
    @JvmStatic external fun attachmentsPending(folderId: Long, uid: Int): Boolean

    // The opener MIME for the stored row, same derivation as the feed's
    // `open_mime`. Read after a download: the magic check may have fixed
    // the stored header since the message was read.
    @JvmStatic external fun attachmentOpenMime(attachmentId: Long): String

    // The viewer copy of a cached attachment, under a safe name in `dir`.
    @JvmStatic external fun writeAttachmentCopy(attachmentId: Long, dir: String): String

    // Filesystem-safe `.eml` name for one message.
    @JvmStatic external fun suggestedEmlName(folderId: Long, uid: Int): String

    // Download-then-assemble as one blocking call (Dart waits for a job).
    @JvmStatic external fun exportEmlBytes(folderId: Long, uid: Int): ByteArray

    // Step 0a shell reads: accounts, folder navigation, outbox pill.
    // JSON or plain strings across, ids back as strings.
    @JvmStatic external fun accountsJson(): String
    @JvmStatic external fun accountForm(id: Long): String
    @JvmStatic external fun accountFormDefaults(): String
    @JvmStatic external fun accountGuess(email: String): String
    @JvmStatic external fun accountPortForSecurity(protocol: String, oldSec: String, newSec: String, port: String): String
    @JvmStatic external fun accountFormCheck(form: String, editing: Boolean): String
    @JvmStatic external fun saveAccount(form: String): String
    // Live IMAP + SMTP login check for the setup form; the infallible JSON
    // report. Blocking: call off the UI thread (Dispatchers.IO).
    @JvmStatic external fun testAccountConnection(form: String): String
    @JvmStatic external fun deleteAccount(id: Long): String
    @JvmStatic external fun initialSelection(): String
    @JvmStatic external fun selectAccount(id: Long): String
    @JvmStatic external fun setFolderSubscribed(folderId: Long, subscribed: Boolean)
    @JvmStatic external fun folderCounts(folderId: Long): String
    @JvmStatic external fun outboxStatusJson(accountId: Long): String

    // Step 0b sync jobs: queue onto mailclient-net, results arrive on the
    // JobCallbacks listener as one JSON event each. Registering replaces
    // the previous listener, so only JobEvents calls these; screens
    // subscribe there.
    @JvmStatic external fun setJobListener(callbacks: JobCallbacks)
    // The core's in-flight job table, {generation, kinds, keys}: what the
    // busy indicator shows. Every job event carries the same snapshot as
    // "busy"; this is for a screen that starts while jobs already run.
    @JvmStatic external fun netBusy(): String
    @JvmStatic external fun syncAccount(accountId: Long)
    @JvmStatic external fun syncFolder(accountId: Long, folderId: Long)
    @JvmStatic external fun loadOlderMessages(accountId: Long, folderId: Long)
    @JvmStatic external fun refreshFolders(accountId: Long)
    @JvmStatic external fun refreshServerCapabilities(accountId: Long)
    @JvmStatic external fun backgroundMarkSeen()
    // Settings: run history in words {last, history}, standby bucket and
    // heartbeat wording, About's {version, license, db_path}.
    @JvmStatic external fun backgroundRunLines(): String
    @JvmStatic external fun limitingBucket(bucket: Int): String
    @JvmStatic external fun heartbeatGap(secs: Long): String
    @JvmStatic external fun appInfoJson(): String

    // Step 0c list reads + bulk mutate. Selections cross as JSON
    // ([1,2,3], [{"folder":"INBOX","uid":1}]); undoable moves answer
    // {"batch","label","purging"}.
    @JvmStatic external fun messagesJson(folderId: Long, limit: Long, offset: Long): String
    // Just these rows (JSON uid array), shaped like messagesJson's: swapped
    // in after a read/star change instead of re-reading the folder.
    @JvmStatic external fun messageRowsJson(folderId: Long, uids: String): String
    @JvmStatic external fun markReadMany(accountId: Long, folderId: Long, uids: String, read: Boolean): String
    @JvmStatic external fun setStarMany(accountId: Long, folderId: Long, uids: String, starred: Boolean): String
    @JvmStatic external fun markReadHits(accountId: Long, hits: String, read: Boolean): String
    @JvmStatic external fun setStarHits(accountId: Long, hits: String, starred: Boolean): String
    @JvmStatic external fun deleteMessages(accountId: Long, folderId: Long, uids: String): String
    @JvmStatic external fun archiveMessages(accountId: Long, folderId: Long, uids: String): String
    @JvmStatic external fun moveMessages(accountId: Long, folderId: Long, uids: String, destPath: String): String
    @JvmStatic external fun purgeMessages(accountId: Long, folderId: Long, uids: String)
    @JvmStatic external fun deleteHits(accountId: Long, hits: String): String
    @JvmStatic external fun archiveHits(accountId: Long, hits: String): String
    @JvmStatic external fun moveHits(accountId: Long, hits: String, destPath: String): String
    @JvmStatic external fun purgeHits(accountId: Long, hits: String)
    @JvmStatic external fun createFolder(accountId: Long, path: String)

    // Step 0d composer/send. Sends and draft saves queue onto
    // mailclient-net (0b events); bad forms throw inline.
    @JvmStatic external fun sendMail(accountId: Long, folderId: Long, form: String)
    @JvmStatic external fun saveDraft(accountId: Long, form: String)
    @JvmStatic external fun draftForm(accountId: Long, uid: Int): String

    // The draft's own files staged under [dir] to re-attach: `[{path,name}]`.
    // Throws while bytes are missing (`missing_files` in draftForm).
    @JvmStatic external fun draftFiles(accountId: Long, uid: Int, dir: String): String
    @JvmStatic external fun deleteDraft(accountId: Long, uid: Int)
    @JvmStatic external fun answerDraft(folderId: Long, uid: Int, mode: String): String

    // A forward's files: how many still need fetching, then the cached ones
    // staged under [dir] as `{files: [{path,name}], missing, notice}`.
    @JvmStatic external fun forwardMissing(folderId: Long, uid: Int): Int
    @JvmStatic external fun forwardFiles(folderId: Long, uid: Int, dir: String): String

    // "Edit & resend" of the bounce (folderId, uid): the same for the sent
    // original it reports on (`answerDraft(.., "resend")` builds the draft).
    @JvmStatic external fun resendMissing(folderId: Long, uid: Int): Int
    @JvmStatic external fun resendFiles(folderId: Long, uid: Int, dir: String): String
    @JvmStatic external fun blankDraft(): String

    // New mail begun outside the app (a mailto: link, a share): the
    // PrefillRequest JSON {mailto?, to?, cc?, bcc?, subject?, text?} in,
    // {to, cc, bcc, subject, body_html} out. Local read.
    @JvmStatic external fun composePrefill(request: String): String

    // Which receipt toggles the composer shows: `{read, delivery}` from the
    // settings (`compose::Receipts::offered`).
    @JvmStatic external fun receiptToggles(): String
    @JvmStatic external fun imageDataUrl(path: String): String
    @JvmStatic external fun senderParts(address: String): String
    @JvmStatic external fun effectiveFrom(local: String, accountEmail: String): String
    // WYSIWYG editor page (mailcore::compose::editor); colours 0xRRGGBB.
    @JvmStatic external fun editorDocument(
        paper: Int, ink: Int, muted: Int, accent: Int, rule: Int, fontPx: Int, placeholder: String, bodyHtml: String,
    ): String
    @JvmStatic external fun composeFormatNote(sendFormat: String, html: String): String

    // Step 0e search/contacts/settings/misc: the last JNI slice.
    @JvmStatic external fun searchJson(accountId: Long, query: String, folder: String): String
    @JvmStatic external fun searchServer(accountId: Long, query: String, folder: String)
    @JvmStatic external fun searchPlan(query: String): String
    // The list filters over search hits: JSON array of kept indexes.
    @JvmStatic external fun listFilterKeep(filterJson: String, rowsJson: String): String
    // The list filters over a folder's cached rows: JSON array of kept uids.
    @JvmStatic external fun listFilterUids(folderId: Long, filterJson: String): String
    // A typed custom date range, normalised or refused: {after, before, error}.
    @JvmStatic external fun dateRangeCheck(after: String, before: String): String
    // The load-older footer's words; server < 0 = never reported.
    @JvmStatic external fun olderLabel(cached: Long, server: Long, filtered: Boolean): String
    @JvmStatic external fun datePresetRange(preset: String): String
    @JvmStatic external fun dateFilterLabel(after: String, before: String): String
    @JvmStatic external fun searchSyntaxHelp(): String
    @JvmStatic external fun similarJson(accountId: Long, folderId: Long, uid: Int): String
    @JvmStatic external fun similarSubject(accountId: Long, folderId: Long, uid: Int): String
    @JvmStatic external fun contactsJson(prefix: String): String
    @JvmStatic external fun setContactAlias(address: String, alias: String)
    @JvmStatic external fun deleteContact(address: String)
    @JvmStatic external fun deleteContacts(addresses: String): String
    @JvmStatic external fun cleanupCandidatesJson(): String
    @JvmStatic external fun recipientSegment(text: String): String
    @JvmStatic external fun replaceRecipientSegment(text: String, replacement: String): String
    @JvmStatic external fun settingsJson(): String
    @JvmStatic external fun settingChoicesJson(): String
    @JvmStatic external fun quietTime(text: String): String
    @JvmStatic external fun quietTimeAt(hour: Int, minute: Int): String
    @JvmStatic external fun setSettings(values: String)
    @JvmStatic external fun setSort(field: String, descending: Boolean)
    @JvmStatic external fun accountSettingsJson(accountId: Long): String
    @JvmStatic external fun setAccountSettings(accountId: Long, values: String): String
    @JvmStatic external fun backgroundPlanJson(): String
    @JvmStatic external fun readerPaint(colored: Boolean, dark: Boolean, keepOriginal: Boolean): String
    @JvmStatic external fun readerPalette(paint: String, paper: Int, ink: Int, link: Int, quote: Int, rule: Int): String
    @JvmStatic external fun readerFitBelow(body: String): String
    @JvmStatic external fun readerDocumentFull(body: String, paint: String, paper: Int, ink: Int, link: Int, quote: Int, rule: Int, allowRemote: Boolean, topSpace: Int, scale: Float, fit: Boolean): String
    @JvmStatic external fun outboxJson(accountId: Long): String
    @JvmStatic external fun dismissOutbox(accountId: Long, id: Long)
    @JvmStatic external fun storageStatsJson(dbPath: String, tempDir: String): String
    @JvmStatic external fun cleanupTempFilesJson(tempDir: String): String
    @JvmStatic external fun pickStageDir(base: String): String
    @JvmStatic external fun trimLocalCache(): String
    @JvmStatic external fun trimStatus(removed: Long): String
    @JvmStatic external fun evictCachedAttachmentsJson(): String
    @JvmStatic external fun exportDatabaseTo(path: String): String
}

// What the net thread calls back with per finished (or progress) job event:
// one JSON object {kind, phase, status, ok, outcome, account_id, folder_id}.
interface JobCallbacks {
    fun onJobEvent(json: String)
}

// What the push monitor calls back on its own thread (MailNative.pushStart).
interface PushCallbacks {
    // Work started (true) or everything is waiting again (false).
    fun onBusy(busy: Boolean)

    // BackgroundReport JSON of a finished check.
    fun onReport(report: String)
}
