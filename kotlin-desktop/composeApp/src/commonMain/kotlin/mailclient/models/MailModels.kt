package mailclient.models

import kotlinx.serialization.Serializable

// Same shapes as mailcore::feed JSON (the QML roles), so the Rust side
// stays the single source of truth for field names and sanitizing.

@Serializable
data class Account(
    val id: Long,
    val name: String,
    val email: String,
    val imap_host: String = "",
    val smtp_host: String = "",
)

@Serializable
data class Folder(
    val id: Long,
    val name: String,
    val role: String = "custom",
    val unread: Long = 0,
    val subscribed: Boolean = true,
    val count: Long = 0,
    val delimiter: String = "/",
) {
    /** Depth for indenting subfolders (QML Folders/MoveTo do the same). */
    fun depth(): Int = if (delimiter.isEmpty()) 0 else name.split(delimiter).size - 1

    fun displayName(): String = name.substringAfterLast(delimiter.ifEmpty { "/" })
}

@Serializable
data class AttachmentMeta(
    val id: Long,
    val filename: String? = null,
    val mime_type: String? = null,
    val size: Long = 0,
    val content_id: String? = null,
    val is_inline: Boolean = false,
)

/** Compact list row (messages_list_json_paged): no bodies, like QML. */
@Serializable
data class MessageRow(
    val uid: Long,
    val subject: String = "(no subject)",
    val from: String = "?",
    val date: String = "",
    val snippet: String = "",
    val unread: Boolean = false,
    val starred: Boolean = false,
    val has_attachments: Boolean = false,
)

/** Full reader payload (message_json): fetched on demand after selection. */
@Serializable
data class MessageDetail(
    val uid: Long,
    val subject: String = "(no subject)",
    val from: String = "?",
    val reply_to: String = "",
    val date: String = "",
    val snippet: String = "",
    val unread: Boolean = false,
    val starred: Boolean = false,
    val has_attachments: Boolean = false,
    val attachments: List<AttachmentMeta> = emptyList(),
    val body_text: String = "",
    val body_html: String = "",
    val is_html: Boolean = false,
    val has_remote_images: Boolean = false,
    val body: String = "",
) {
    /** Plain readable text: real text part, else HTML stripped of tags. */
    fun readableText(): String {
        if (body_text.isNotBlank()) return body_text
        if (body.isNotBlank() && !is_html) return body
        val html = body_html.ifBlank { body }
        if (html.isBlank()) return "(empty)"
        return html
            .replace(Regex("(?i)<br\\s*/?>"), "\n")
            .replace(Regex("(?i)</p\\s*>"), "\n\n")
            .replace(Regex("<[^>]*>"), "")
            .replace("&nbsp;", " ")
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .lines().joinToString("\n") { it.trimEnd() }
            .trim()
            .ifBlank { "(no displayable content)" }
    }
}

@Serializable
data class SearchHit(
    val uid: Long,
    val folder_id: Long = 0,
    val folder: String = "",
    val subject: String = "(no subject)",
    val from: String = "?",
    val date: String = "",
    val snippet: String = "",
    val unread: Boolean = false,
    val starred: Boolean = false,
    val has_attachments: Boolean = false,
)

@Serializable
data class AccountStatus(
    val account_id: Long,
    val email: String = "",
    val unread: Long = 0,
    val fetched: Long = 0,
    val expunged: Long = 0,
    val errors: List<String> = emptyList(),
)

@Serializable
data class RecentMail(
    val account_id: Long = 0,
    val account_email: String = "",
    val folder: String = "",
    val from: String = "",
    val subject: String = "",
    val date: String = "",
)

@Serializable
data class StatusReport(
    val ok: Boolean = false,
    val unread: Long = 0,
    val accounts: List<AccountStatus> = emptyList(),
    val recent: List<RecentMail> = emptyList(),
)

@Serializable
data class SyncReport(
    val ok: Boolean = false,
    val locked: Boolean = false,
    val unread: Long = 0,
    val fetched: Long = 0,
    val expunged: Long = 0,
    val accounts: List<AccountStatus> = emptyList(),
    val errors: List<String> = emptyList(),
)
