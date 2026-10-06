package de.renier.mailclient.ui.composer

import de.renier.mailclient.MailNative
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
 * (`mailcore::compose::answer`, `open_draft`) so the screen only edits text.
 * Flutter's ComposerInitial, field for field.
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
    val body: String = "",
    val draftUid: Int = -1,
    // Filenames the server copy of a draft holds: saving replaces that copy,
    // so they are called out rather than silently dropped.
    val serverAttachments: List<String> = emptyList(),
    // Answering mail whose replies go somewhere unexpected.
    val replyNotice: String = "",
    // The quoted original as HTML, carried beside the text box.
    val quoteHtml: String = "",
    val quoteFirst: Boolean = false,
) {
    companion object {
        // Blocking JNI reads (local SQLite only): call off the main thread.

        /** New mail, with the signature below room to type. */
        fun blank(): ComposerSeed =
            ComposerSeed(ComposeMode.Blank, body = bodyFor(JSONObject(MailNative.blankDraft())))

        /** Reply, reply-all or forward: recipients, subject, quote prepared by the core. */
        fun answer(folderId: Long, uid: Int, mode: ComposeMode): ComposerSeed {
            val wire = when (mode) {
                ComposeMode.ReplyAll -> "reply_all"
                ComposeMode.Forward -> "forward"
                else -> "reply"
            }
            val d = JSONObject(MailNative.answerDraft(folderId, uid, wire))
            val noticeAddr = d.optString("notice_addr")
            return ComposerSeed(
                mode = mode,
                to = d.optString("to"),
                cc = d.optString("cc"),
                subject = d.optString("subject"),
                body = bodyFor(d),
                replyNotice = if (noticeAddr.isEmpty()) {
                    ""
                } else {
                    "Replies to this mail go to $noticeAddr — not to the sender (${d.optString("notice_sender")})."
                },
                quoteHtml = d.optString("quote_html"),
                quoteFirst = d.optBoolean("quote_first"),
            )
        }

        /** Continue a stored draft of the Drafts folder. */
        fun draft(accountId: Long, uid: Int): ComposerSeed {
            val f = JSONObject(MailNative.draftForm(accountId, uid))
            val files = f.optJSONArray("attachments")
            val names = buildList {
                for (i in 0 until (files?.length() ?: 0)) {
                    val a = files!!.optJSONObject(i) ?: continue
                    if (!a.optBoolean("is_inline")) add(a.optString("filename"))
                }
            }
            val text = f.optString("body")
            return ComposerSeed(
                mode = ComposeMode.Draft,
                fromAddr = f.optString("from"),
                to = f.optString("to"),
                cc = f.optString("cc"),
                bcc = f.optString("bcc"),
                replyTo = f.optString("reply_to"),
                subject = f.optString("subject"),
                body = text.ifEmpty { f.optString("body_html") },
                draftUid = f.optInt("draft_uid", uid),
                serverAttachments = names,
            )
        }

        private fun bodyFor(d: JSONObject): String {
            val sig = d.optString("signature_text")
            return if (sig.isEmpty()) "" else "\n\n$sig"
        }
    }
}
