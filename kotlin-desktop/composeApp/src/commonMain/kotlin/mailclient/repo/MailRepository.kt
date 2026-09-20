package mailclient.repo

import mailclient.models.Account
import mailclient.models.Folder
import mailclient.models.MessageDetail
import mailclient.models.MessageRow
import mailclient.models.SearchHit
import mailclient.models.StatusReport
import mailclient.models.SyncReport

/**
 * Backend seam. The desktop app uses the `mailfeed` CLI (JVM process);
 * the future Android app implements the same interface over JNI/UniFFI
 * against `mailcore` directly — all screens in commonMain stay untouched.
 */
interface MailRepository {
    suspend fun accounts(): List<Account>
    suspend fun folders(accountId: Long): List<Folder>
    suspend fun messages(folderId: Long, limit: Int = 200, offset: Int = 0): List<MessageRow>
    suspend fun message(folderId: Long, uid: Long): MessageDetail
    suspend fun search(accountId: Long, query: String, folder: String = "", limit: Int = 50): List<SearchHit>
    suspend fun markRead(folderId: Long, uid: Long, read: Boolean = true)
    suspend fun markStar(folderId: Long, uid: Long, starred: Boolean)
    suspend fun status(accountId: Long? = null): StatusReport
    suspend fun sync(accountId: Long? = null): SyncReport
    suspend fun send(
        accountId: Long,
        to: String,
        cc: String = "",
        bcc: String = "",
        subject: String,
        body: String,
        bodyHtml: String? = null,
        replyTo: String? = null,
        attachments: List<String> = emptyList(),
    )
    suspend fun delete(folderId: Long, uid: Long)
    suspend fun archive(folderId: Long, uid: Long)
    suspend fun openAttachment(attachmentId: Long): String
}
