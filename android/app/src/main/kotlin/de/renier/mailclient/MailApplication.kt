package de.renier.mailclient

import android.app.Application
import java.io.File
import kotlin.concurrent.thread

// Installs the crash capture before any activity, service or receiver
// runs: every one of them starts with Application.onCreate in the same
// process, so a single install here covers the composer, the background
// worker and the push service. Reports stay on-device under `crashes/`
// until shared from Settings → Maintenance.
class MailApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        CrashLog.install(this)
        sweepCache()
        initCore()
    }

    // Open the core (data dir, database, migrations) as the process starts,
    // off the main thread: every screen, worker and receiver then finds it
    // ready, and none can reach the database before the data dir is set.
    // Callers still call ensureInit; it returns at once once this is done.
    private fun initCore() {
        thread(name = "core-init", isDaemon = true) {
            runCatching { MailNative.ensureInit(this) }
                .onFailure { android.util.Log.w("mailclient", "core init failed", it) }
        }
    }

    // Leftovers nothing in a fresh process can still be using: the old
    // `outgoing/` staging folder (picks and drafts now stage in the core's
    // prefixed dirs, pruned by the core) and a database export cut short
    // by the process dying before its temp copy was deleted.
    private fun sweepCache() {
        val cache = cacheDir
        thread(name = "cache-sweep", isDaemon = true) {
            runCatching { File(cache, "outgoing").deleteRecursively() }
            cache.listFiles { f -> f.isFile && f.name.startsWith("export-") && f.name.endsWith(".sqlite") }
                ?.forEach { runCatching { it.delete() } }
        }
    }
}
