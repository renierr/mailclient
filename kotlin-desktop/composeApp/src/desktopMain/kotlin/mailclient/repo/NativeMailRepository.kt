package mailclient.repo

import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import mailclient.models.*

/**
 * In-process native [MailRepository] over JNI (`libmailcore_jni.so`).
 * Runs directly in the JVM process without process spawning overhead,
 * maintaining persistent SQLite and Tokio runtime sessions.
 */
class NativeMailRepository(dbPath: String? = NativeLoader.devDbPath()) : MailRepository {
    companion object {
        init {
            NativeLoader.load()
        }
    }

    init {
        nativeInit(dbPath)
    }

    private val json = Json { ignoreUnknownKeys = true; isLenient = true }

    private external fun nativeInit(dbPath: String?)
    private external fun nativeAccounts(): String
    private external fun nativeFolders(accountId: Long): String
    private external fun nativeMessages(folderId: Long, limit: Int, offset: Int): String
    private external fun nativeMessage(folderId: Long, uid: Long): String
    private external fun nativeSearch(accountId: Long, query: String, folder: String, limit: Int): String
    private external fun nativeMarkRead(folderId: Long, uid: Long, read: Boolean)
    private external fun nativeMarkStar(folderId: Long, uid: Long, starred: Boolean)
    private external fun nativeStatus(accountId: Long): String
    private external fun nativeSync(accountId: Long): String
    private external fun nativeSend(
        accountId: Long,
        to: String,
        cc: String,
        bcc: String,
        subject: String,
        body: String,
        bodyHtml: String?,
        replyTo: String?,
        attachments: String,
    ): String
    private external fun nativeDelete(folderId: Long, uid: Long): String
    private external fun nativeArchive(folderId: Long, uid: Long): String
    private external fun nativeOpenAttachment(attachmentId: Long): String

    override suspend fun accounts(): List<Account> = withContext(Dispatchers.IO) {
        val res = nativeAccounts()
        json.decodeFromString(ListSerializer(Account.serializer()), res)
    }

    override suspend fun folders(accountId: Long): List<Folder> = withContext(Dispatchers.IO) {
        val res = nativeFolders(accountId)
        json.decodeFromString(ListSerializer(Folder.serializer()), res)
    }

    override suspend fun messages(folderId: Long, limit: Int, offset: Int): List<MessageRow> = withContext(Dispatchers.IO) {
        val res = nativeMessages(folderId, limit, offset)
        json.decodeFromString(ListSerializer(MessageRow.serializer()), res)
    }

    override suspend fun message(folderId: Long, uid: Long): MessageDetail = withContext(Dispatchers.IO) {
        val res = nativeMessage(folderId, uid)
        json.decodeFromString(MessageDetail.serializer(), res)
    }

    override suspend fun search(accountId: Long, query: String, folder: String, limit: Int): List<SearchHit> = withContext(Dispatchers.IO) {
        if (query.isBlank()) return@withContext emptyList()
        val res = nativeSearch(accountId, query, folder, limit)
        json.decodeFromString(ListSerializer(SearchHit.serializer()), res)
    }

    override suspend fun markRead(folderId: Long, uid: Long, read: Boolean) = withContext(Dispatchers.IO) {
        nativeMarkRead(folderId, uid, read)
    }

    override suspend fun markStar(folderId: Long, uid: Long, starred: Boolean) = withContext(Dispatchers.IO) {
        nativeMarkStar(folderId, uid, starred)
    }

    override suspend fun status(accountId: Long?): StatusReport = withContext(Dispatchers.IO) {
        val res = nativeStatus(accountId ?: -1)
        json.decodeFromString(StatusReport.serializer(), res)
    }

    override suspend fun sync(accountId: Long?): SyncReport = withContext(Dispatchers.IO) {
        val res = nativeSync(accountId ?: -1)
        json.decodeFromString(SyncReport.serializer(), res)
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
    ) = withContext(Dispatchers.IO) {
        val res = nativeSend(
            accountId, to, cc, bcc, subject, body,
            bodyHtml, replyTo, attachments.joinToString(";")
        )
        val obj = json.parseToJsonElement(res)
        if (obj is kotlinx.serialization.json.JsonObject) {
            val ok = (obj["ok"] as? kotlinx.serialization.json.JsonPrimitive)?.content == "true"
            if (!ok) {
                val err = (obj["error"] as? kotlinx.serialization.json.JsonPrimitive)?.content ?: "send failed"
                throw IllegalStateException(err)
            }
        }
    }

    override suspend fun delete(folderId: Long, uid: Long) = withContext(Dispatchers.IO) {
        val res = nativeDelete(folderId, uid)
        val obj = json.parseToJsonElement(res)
        if (obj is kotlinx.serialization.json.JsonObject) {
            val ok = (obj["ok"] as? kotlinx.serialization.json.JsonPrimitive)?.content == "true"
            if (!ok) {
                val err = (obj["error"] as? kotlinx.serialization.json.JsonPrimitive)?.content ?: "delete failed"
                throw IllegalStateException(err)
            }
        }
    }

    override suspend fun archive(folderId: Long, uid: Long) = withContext(Dispatchers.IO) {
        val res = nativeArchive(folderId, uid)
        val obj = json.parseToJsonElement(res)
        if (obj is kotlinx.serialization.json.JsonObject) {
            val ok = (obj["ok"] as? kotlinx.serialization.json.JsonPrimitive)?.content == "true"
            if (!ok) {
                val err = (obj["error"] as? kotlinx.serialization.json.JsonPrimitive)?.content ?: "archive failed"
                throw IllegalStateException(err)
            }
        }
    }

    override suspend fun openAttachment(attachmentId: Long): String = withContext(Dispatchers.IO) {
        val res = nativeOpenAttachment(attachmentId)
        val obj = json.parseToJsonElement(res)
        if (obj is kotlinx.serialization.json.JsonObject) {
            val ok = (obj["ok"] as? kotlinx.serialization.json.JsonPrimitive)?.content == "true"
            if (ok) {
                return@withContext (obj["path"] as? kotlinx.serialization.json.JsonPrimitive)?.content
                    ?: throw IllegalStateException("missing path")
            }
            val err = (obj["error"] as? kotlinx.serialization.json.JsonPrimitive)?.content ?: "open attachment failed"
            throw IllegalStateException(err)
        }
        throw IllegalStateException("unexpected response")
    }
}

/** Locates and loads `libmailcore_jni.so` */
object NativeLoader {
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

    fun load() {
        System.getenv("MAILCORE_JNI_LIB")?.takeIf { File(it).isFile }?.let {
            System.load(it)
            return
        }

        try {
            System.loadLibrary("mailcore_jni")
            return
        } catch (_: UnsatisfiedLinkError) {
        }

        val cwd = File(System.getProperty("user.dir"))
        var dir: File? = cwd
        repeat(4) {
            dir?.let {
                for (profile in listOf("release", "debug")) {
                    for (name in listOf("libmailcore_jni.so", "mailcore_jni.dll", "libmailcore_jni.dylib")) {
                        val lib = File(it, "target/$profile/$name")
                        if (lib.isFile) {
                            System.load(lib.absolutePath)
                            return
                        }
                    }
                }
            }
            dir = dir?.parentFile
        }

        throw UnsatisfiedLinkError("Could not locate libmailcore_jni.so. Run `cargo build -p mailjni --release` first.")
    }
}
