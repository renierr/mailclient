package de.renier.mailclient

import android.content.Context
import java.util.concurrent.CopyOnWriteArrayList

// Process-lifetime fan-out for job events. The JNI side holds exactly one
// listener and a new one replaces the old, so screens never touch
// MailNative.setJobListener themselves: they subscribe here, and closing one
// screen cannot silence another. Callbacks run on Rust's net thread.
object JobEvents {
    private val subscribers = CopyOnWriteArrayList<(String) -> Unit>()

    @Volatile private var registered = false

    /** Subscribe [onEvent]; close the result to unsubscribe. */
    fun subscribe(context: Context, onEvent: (String) -> Unit): AutoCloseable {
        ensureRegistered(context)
        subscribers.add(onEvent)
        return AutoCloseable { subscribers.remove(onEvent) }
    }

    private fun ensureRegistered(context: Context) {
        if (registered) return
        synchronized(this) {
            if (registered) return
            MailNative.ensureInit(context)
            MailNative.setJobListener(
                object : JobCallbacks {
                    override fun onJobEvent(json: String) {
                        for (s in subscribers) runCatching { s(json) }
                    }
                },
            )
            registered = true
        }
    }
}
