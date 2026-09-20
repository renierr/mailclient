package mailclient.repo

import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import mailclient.models.Account
import mailclient.models.Folder
import mailclient.models.MessageDetail
import mailclient.models.MessageRow
import mailclient.models.SearchHit
import mailclient.models.StatusReport
import mailclient.models.SyncReport

/**
 * [MailRepository] over the `mailfeed` CLI (stdout JSON, see
 * crates/mailfeed). All blocking process I/O stays on Dispatchers.IO —
 * never on the Compose UI thread.
 */
class CliMailRepository(private val mailfeedBin: String) : MailRepository {
    private val json = Json { ignoreUnknownKeys = true; isLenient = true }

    private suspend fun run(vararg args: String): String = withContext(Dispatchers.IO) {
        val pb = ProcessBuilder(listOf(mailfeedBin) + args)
            .redirectErrorStream(false)
        MailfeedLocator.repoDir()?.let { pb.directory(it) }
        MailfeedLocator.devDbPath()?.let { pb.environment()["MAILCLIENT_DB"] = it }
        val proc = pb.start()
        val out = proc.inputStream.bufferedReader().readText()
        val err = proc.errorStream.bufferedReader().readText()
        val code = proc.waitFor()
        if (code != 0) throw IllegalStateException("mailfeed ${args.firstOrNull()} failed: ${err.take(300)}")
        out
    }

    override suspend fun accounts(): List<Account> =
        json.decodeFromString(ListSerializer(Account.serializer()), run("accounts").trim())

    override suspend fun folders(accountId: Long): List<Folder> =
        json.decodeFromString(ListSerializer(Folder.serializer()), run("folders", "--account", "$accountId").trim())

    override suspend fun messages(folderId: Long, limit: Int, offset: Int): List<MessageRow> =
        json.decodeFromString(
            ListSerializer(MessageRow.serializer()),
            run("messages", "--folder", "$folderId", "--limit", "$limit", "--offset", "$offset").trim(),
        )

    override suspend fun message(folderId: Long, uid: Long): MessageDetail =
        json.decodeFromString(MessageDetail.serializer(), run("message", "--folder", "$folderId", "--uid", "$uid").trim())

    override suspend fun search(accountId: Long, query: String, folder: String, limit: Int): List<SearchHit> {
        if (query.isBlank()) return emptyList()
        val args = mutableListOf("search", "--account", "$accountId", "--query", query, "--limit", "$limit")
        if (folder.isNotEmpty()) {
            args += listOf("--folder", folder)
        }
        return json.decodeFromString(ListSerializer(SearchHit.serializer()), run(*args.toTypedArray()).trim())
    }

    override suspend fun markRead(folderId: Long, uid: Long, read: Boolean) {
        val args = mutableListOf("mark-read", "--folder", "$folderId", "--uid", "$uid")
        if (!read) args += "--unread"
        run(*args.toTypedArray())
    }

    override suspend fun markStar(folderId: Long, uid: Long, starred: Boolean) {
        val args = mutableListOf("mark-star", "--folder", "$folderId", "--uid", "$uid")
        if (!starred) args += "--off"
        run(*args.toTypedArray())
    }

    override suspend fun status(accountId: Long?): StatusReport {
        val args = mutableListOf("status")
        if (accountId != null) args += listOf("--account", "$accountId")
        return json.decodeFromString(StatusReport.serializer(), run(*args.toTypedArray()).trim())
    }

    override suspend fun sync(accountId: Long?): SyncReport {
        val args = mutableListOf("sync")
        if (accountId != null) args += listOf("--account", "$accountId")
        return json.decodeFromString(SyncReport.serializer(), run(*args.toTypedArray()).trim())
    }

    override suspend fun send(
        accountId: Long,
        to: String,
        cc: String,
        bcc: String,
        subject: String,
        body: String,
        bodyHtml: String?,
        replyTo: String?,
        attachments: List<String>,
    ) {
        val args = mutableListOf(
            "send",
            "--account", "$accountId",
            "--to", to,
            "--subject", subject,
            "--body", body,
        )
        if (cc.isNotBlank()) args += listOf("--cc", cc)
        if (bcc.isNotBlank()) args += listOf("--bcc", bcc)
        if (!bodyHtml.isNullOrBlank()) args += listOf("--body-html", bodyHtml)
        if (!replyTo.isNullOrBlank()) args += listOf("--reply-to", replyTo)
        if (attachments.isNotEmpty()) args += listOf("--attachments", attachments.joinToString(";"))
        run(*args.toTypedArray())
    }

    override suspend fun delete(folderId: Long, uid: Long) {
        run("delete", "--folder", "$folderId", "--uid", "$uid")
    }

    override suspend fun archive(folderId: Long, uid: Long) {
        run("archive", "--folder", "$folderId", "--uid", "$uid")
    }

    override suspend fun openAttachment(attachmentId: Long): String {
        val out = run("open-attachment", "--id", "$attachmentId")
        val obj = json.parseToJsonElement(out)
        return obj.let { it as? kotlinx.serialization.json.JsonObject }
            ?.get("path")
            ?.let { (it as? kotlinx.serialization.json.JsonPrimitive)?.content }
            ?: throw IllegalStateException("could not extract path from open-attachment")
    }
}

/** Locate the `mailfeed` binary: env, repo target dirs, then PATH. */
object MailfeedLocator {
    fun repoDir(): File? {
        val cwd = File(System.getProperty("user.dir"))
        var dir: File? = cwd
        repeat(4) {
            dir?.let {
                if (File(it, "Cargo.toml").exists() && File(it, "crates").isDirectory) return it
            }
            dir = dir?.parentFile
        }
        return null
    }

    fun devDbPath(): String? {
        val envDb = System.getenv("MAILCLIENT_DB")
        if (!envDb.isNullOrBlank()) return envDb
        val envFile = File(repoDir() ?: return null, ".env")
        if (envFile.isFile) {
            envFile.readLines().forEach { line ->
                val trimmed = line.trim()
                if (trimmed.startsWith("MAILCLIENT_DB=")) {
                    return trimmed.substringAfter("MAILCLIENT_DB=").trim().trim('"', '\'')
                }
            }
        }
        return null
    }

    fun find(): String {
        System.getenv("MAILFEED_BIN")?.takeIf { File(it).canExecute() }?.let { return it }
        val cwd = File(System.getProperty("user.dir"))
        // Gradle runs with user.dir = composeApp module dir or project dir.
        var dir: File? = cwd
        repeat(4) {
            dir?.let {
                for (profile in listOf("release", "debug")) {
                    val cand = File(it, "target/$profile/mailfeed")
                    if (cand.canExecute()) return cand.absolutePath
                }
            }
            dir = dir?.parentFile
        }
        return "mailfeed" // PATH fallback
    }
}
