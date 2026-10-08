package de.renier.mailclient

import android.content.Context
import android.os.Build
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/**
 * Last-resort crash capture for installs without adb (e.g. the signed
 * release on a phone). Installed once per process start
 * ([MailApplication]): any uncaught exception — the forward/composer crash
 * included — is written to the app-private `crashes/` directory before the
 * previous handler takes over and kills the process. [MaintenanceSection]
 * lists, shares and deletes these files; nothing leaves the device except
 * through the explicit Share/Save buttons there.
 *
 * What it cannot see: a native abort straight out of Rust (a panic across
 * JNI or a SIGSEGV) never passes through this Java handler. The JNI layer
 * converts core failures into Java exceptions
 * (`ThrowRuntimeExAndDefault`), so those do land here.
 */
object CrashLog {
    private const val DIR = "crashes"
    private const val MAX_FILES = 10

    @Volatile private var installed = false

    fun install(context: Context) {
        if (installed) return
        synchronized(this) {
            if (installed) return
            val app = context.applicationContext
            val prev = Thread.getDefaultUncaughtExceptionHandler()
            Thread.setDefaultUncaughtExceptionHandler { thread, error ->
                runCatching { write(app, thread, error) }
                if (prev != null) {
                    prev.uncaughtException(thread, error)
                } else {
                    android.os.Process.killProcess(android.os.Process.myPid())
                    kotlin.system.exitProcess(10)
                }
            }
            installed = true
        }
    }

    fun dir(context: Context): File = File(context.filesDir, DIR)

    /** Newest first, capped at [MAX_FILES]. Missing dir reads as none. */
    fun list(context: Context): List<File> =
        runCatching {
            val d = dir(context)
            if (!d.isDirectory) return emptyList()
            d.listFiles { f -> f.isFile && f.name.endsWith(".log") }
                ?.sortedByDescending { it.lastModified() }
                ?.take(MAX_FILES)
                .orEmpty()
        }.getOrDefault(emptyList())

    fun deleteAll(context: Context) {
        runCatching { dir(context).deleteRecursively() }
    }

    fun describe(file: File): String {
        val date = SimpleDateFormat.getDateTimeInstance(
            SimpleDateFormat.MEDIUM, SimpleDateFormat.SHORT, Locale.getDefault(),
        ).format(Date(file.lastModified()))
        return "${file.name} · ${file.length() / 1024} KB · $date"
    }

    private fun write(context: Context, thread: Thread, error: Throwable) {
        val d = dir(context)
        d.mkdirs()
        val stamp = SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).format(Date())
        val file = File(d, "crash-$stamp.log")
        val version = runCatching {
            val pm = context.packageManager
            val info = pm.getPackageInfo(context.packageName, 0)
            "${info.versionName} (${info.versionCode})"
        }.getOrDefault("unknown")
        val header = buildString {
            appendLine("mailclient crash ${SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ssZ", Locale.US).format(Date())}")
            appendLine("app: $version")
            appendLine("device: ${Build.MANUFACTURER} ${Build.MODEL} (Android ${Build.VERSION.RELEASE}, SDK ${Build.VERSION.SDK_INT}, ${Build.SUPPORTED_ABIS.firstOrNull() ?: "?"})")
            appendLine("thread: ${thread.name}")
            appendLine()
        }
        // Already carries the "Caused by:" chain.
        file.writeText(header + error.stackTraceToString())
        // Keep the directory bounded: the oldest beyond the cap go.
        runCatching {
            d.listFiles { f -> f.isFile && f.name.endsWith(".log") }
                ?.sortedByDescending { it.lastModified() }
                ?.drop(MAX_FILES)
                ?.forEach { it.delete() }
        }
    }
}
