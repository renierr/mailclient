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

    // One scheduled check; blocks for the network run. BackgroundReport JSON.
    @JvmStatic external fun check(trigger: String): String

    // NotificationPlan JSON for a report. `shown`: JSON object of the mail
    // notifications on screen, tag -> signature.
    @JvmStatic external fun plan(report: String, permitted: Boolean, foreground: Boolean, shown: String): String

    // A "Mark read" button: mark the ReadTarget JSON read in the cache, no
    // network. BackgroundReport JSON of what is still pending, for plan().
    @JvmStatic external fun markRead(target: String): String

    // Send an account's queued flag changes; blocks for the network, throws
    // when the server cannot be reached.
    @JvmStatic external fun pushFlags(accountId: Long)

    // The plan was carried out: commit its marks and outcome.
    @JvmStatic external fun commit(plan: String)

    @JvmStatic external fun recordOutcome(run: String, outcome: String)

    @JvmStatic external fun pushStart(callbacks: PushCallbacks, online: Boolean)

    @JvmStatic external fun pushKeepalive()

    @JvmStatic external fun pushNetwork(online: Boolean)

    @JvmStatic external fun pushStop()
}

// What the push monitor calls back on its own thread (MailNative.pushStart).
interface PushCallbacks {
    // Work started (true) or everything is waiting again (false).
    fun onBusy(busy: Boolean)

    // BackgroundReport JSON of a finished check.
    fun onReport(report: String)
}
