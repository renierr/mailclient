package de.renier.mailclient.ui.composer

import android.content.Context
import android.net.Uri
import de.renier.mailclient.MailNative
import de.renier.mailclient.ui.common.objects
import org.json.JSONArray
import org.json.JSONObject

/** How the composer was opened: what it prefills and what Send replaces. */
enum class ComposeMode(val title: String) {
    Blank("New message"),
    Reply("Reply"),
    ReplyAll("Reply all"),
    Forward("Forward"),
    Resend("Edit & resend"),
    Draft("Edit draft"),
}

/**
 * The composer's starting point, built from the core's drafts
 * (`mailcore::compose::answer`, `draft_editor_html`) so the screen only
 * edits. [bodyHtml] is HTML for the WYSIWYG editor, the way Qt seeds its
 * EditorFrame: text slot, signature and quote already in place.
 */
data class ComposerSeed(
    val mode: ComposeMode,
    // A reopened draft's stored From; new mail sends as the account.
    val fromAddr: String = "",
    val to: String = "",
    val cc: String = "",
    val bcc: String = "",
    val replyTo: String = "",
    val subject: String = "",
    val bodyHtml: String = "",
    val draftUid: Int = -1,
    // Filenames the server copy of a draft holds: saving replaces that copy,
    // so they are called out rather than silently dropped.
    val serverAttachments: List<String> = emptyList(),
    // Answering mail whose replies go somewhere unexpected.
    val replyNotice: String = "",
    val replyNoticeAddr: String = "",
    // A forward's files that could not be fetched: the core's sentence.
    val filesNotice: String = "",
    // A send that failed after the composer closed comes back with
    // everything as it was sent: sender name, picked files, and why.
    val fromName: String? = null,
    val attachments: List<PickedFile> = emptyList(),
    val failure: String = "",
    // Receipts the failed send asked for; null starts from the settings.
    val requestMdn: Boolean? = null,
    val requestDsn: Boolean? = null,
) {
    companion object {
        // Blocking JNI reads (local SQLite only): call off the main thread.

        /** New mail, with the signature below room to type. */
        fun blank(): ComposerSeed =
            ComposerSeed(ComposeMode.Blank, bodyHtml = JSONObject(MailNative.blankDraft()).optString("body_html"))

        /**
         * Reply or reply-all: recipients, subject, quote prepared by the
         * core. Blocking JNI read: call off the main thread. Forward,
         * resend and draft opens go through the fetch jobs
         * (`MailNative.forwardFetch` and friends) and assemble with
         * [assembleAnswer]/[assembleDraft] — replies fetch nothing.
         */
        fun answer(folderId: Long, uid: Int, mode: ComposeMode): ComposerSeed {
            val wire = if (mode == ComposeMode.ReplyAll) "reply_all" else "reply"
            val d = JSONObject(MailNative.answerDraft(folderId, uid, wire))
            return assembleAnswer(mode, d, null)
        }

        /**
         * The assembly behind [answer] and the forward/resend fetch jobs:
         * the answer draft plus the staged files, if any.
         */
        fun assembleAnswer(mode: ComposeMode, d: JSONObject, fwd: JSONObject?): ComposerSeed {
            val files = fwd?.optJSONArray("files")
            return ComposerSeed(
                mode = mode,
                // A reply answers from the alias the mail was delivered to.
                fromAddr = d.optString("from"),
                to = d.optString("to"),
                cc = d.optString("cc"),
                subject = d.optString("subject"),
                bodyHtml = d.optString("body_html"),
                // The core's sentence; shown while To still holds the address.
                replyNotice = d.optString("notice"),
                replyNoticeAddr = d.optString("notice_addr"),
                filesNotice = fwd?.optString("notice").orEmpty(),
                attachments = files.objects().map { PickedFile(it.getString("path"), it.getString("name")) },
            )
        }

        /**
         * New mail begun outside the app (ComposeEntryActivity's payload:
         * the core's PrefillRequest plus shared content URIs): recipients,
         * subject and body from the core, the files copied in like picked
         * ones while this activity still holds their read grant.
         */
        fun prefill(context: Context, payload: String): ComposerSeed {
            val p = JSONObject(payload)
            val d = JSONObject(MailNative.composePrefill(p.optJSONObject("request")?.toString() ?: "{}"))
            val shared = p.optJSONArray("files")?.let { a -> List(a.length()) { Uri.parse(a.getString(it)) } }.orEmpty()
            val files = shared.mapNotNull { ComposerFiles.copyIn(context, it) }
            val missed = shared.size - files.size
            return ComposerSeed(
                mode = ComposeMode.Blank,
                to = d.optString("to"),
                cc = d.optString("cc"),
                bcc = d.optString("bcc"),
                subject = d.optString("subject"),
                bodyHtml = d.optString("body_html"),
                attachments = files,
                filesNotice = when (missed) {
                    0 -> ""
                    1 -> "1 shared file could not be attached."
                    else -> "$missed shared files could not be attached."
                },
            )
        }

        /**
         * Continue a stored draft of the Drafts folder, from a fetch job's
         * `{form, staged}` seed: the form plus its files staged for
         * re-attaching. A null `staged` names the server copy's files
         * instead, so saving does not drop them silently.
         */
        fun assembleDraft(f: JSONObject, uid: Int, staged: List<PickedFile>?): ComposerSeed {
            val files = f.optJSONArray("attachments")
            val names = if (staged != null) {
                emptyList()
            } else {
                files.objects().filter { !it.optBoolean("is_inline") }.map { it.optString("filename") }
            }
            return ComposerSeed(
                mode = ComposeMode.Draft,
                fromAddr = f.optString("from"),
                to = f.optString("to"),
                cc = f.optString("cc"),
                bcc = f.optString("bcc"),
                replyTo = f.optString("reply_to"),
                subject = f.optString("subject"),
                bodyHtml = f.optString("editor_html"),
                draftUid = f.optInt("draft_uid", uid),
                serverAttachments = names,
                attachments = staged.orEmpty(),
            )
        }
    }
}
