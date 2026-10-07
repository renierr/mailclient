package de.renier.mailclient.ui.composer

import de.renier.mailclient.MailNative
import org.json.JSONArray
import org.json.JSONObject

/** How the composer was opened: what it prefills and what Send replaces. */
enum class ComposeMode(val title: String) {
    Blank("New message"),
    Reply("Reply"),
    ReplyAll("Reply all"),
    Forward("Forward"),
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
    // A send that failed after the composer closed comes back with
    // everything as it was sent: sender name, picked files, and why.
    val fromName: String? = null,
    val attachments: List<PickedFile> = emptyList(),
    val failure: String = "",
) {
    companion object {
        // Blocking JNI reads (local SQLite only): call off the main thread.

        /** New mail, with the signature below room to type. */
        fun blank(): ComposerSeed =
            ComposerSeed(ComposeMode.Blank, bodyHtml = JSONObject(MailNative.blankDraft()).optString("body_html"))

        /** Reply, reply-all or forward: recipients, subject, quote prepared by the core. */
        fun answer(folderId: Long, uid: Int, mode: ComposeMode): ComposerSeed {
            val wire = when (mode) {
                ComposeMode.ReplyAll -> "reply_all"
                ComposeMode.Forward -> "forward"
                else -> "reply"
            }
            val d = JSONObject(MailNative.answerDraft(folderId, uid, wire))
            return ComposerSeed(
                mode = mode,
                to = d.optString("to"),
                cc = d.optString("cc"),
                subject = d.optString("subject"),
                bodyHtml = d.optString("body_html"),
                // The core's sentence; shown while To still holds the address.
                replyNotice = d.optString("notice"),
                replyNoticeAddr = d.optString("notice_addr"),
            )
        }

        /**
         * Continue a stored draft of the Drafts folder. Its files are staged
         * under [stageDir] and re-attached (Qt does the same); only when
         * their bytes could not be fetched are they named instead, so saving
         * does not drop them silently.
         */
        fun draft(accountId: Long, uid: Int, stageDir: String): ComposerSeed {
            val f = JSONObject(MailNative.draftForm(accountId, uid))
            val staged = runCatching {
                val a = JSONArray(MailNative.draftFiles(accountId, uid, stageDir))
                List(a.length()) { i -> a.getJSONObject(i).let { PickedFile(it.getString("path"), it.getString("name")) } }
            }.getOrNull()
            val files = f.optJSONArray("attachments")
            val names = if (staged != null) emptyList() else buildList {
                for (i in 0 until (files?.length() ?: 0)) {
                    val a = files!!.optJSONObject(i) ?: continue
                    if (!a.optBoolean("is_inline")) add(a.optString("filename"))
                }
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
