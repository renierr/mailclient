package de.renier.mailclient

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.widget.Toast
import org.json.JSONObject

/**
 * The one-line answer to a check the user started themselves — the "Check
 * mail" Quick Settings tile. New mail answers through its notifications,
 * but a check that finds nothing (or fails) would otherwise say nothing at
 * all: the tile's busy state is the only other sign of life, and once.
 *
 * Only ever called for the tile's tap, so a scheduled check never toasts.
 */
internal object CheckFeedback {
    /**
     * The last tap's answer, until the tile has shown it once: the tile's
     * own subtitle is the feedback that always works, while a toast can be
     * hidden or blocked on a phone.
     */
    @Volatile
    private var pendingOutcome: String? = null

    fun show(context: Context, report: String) {
        pendingOutcome = outcome(report)
        toast(context, pendingOutcome ?: return)
    }

    /** The check blew up before the core could report anything. */
    fun showFailure(context: Context) {
        pendingOutcome = "Mail check failed"
        toast(context, pendingOutcome!!)
    }

    /** The answer a tap is still owed, taken once. */
    fun takeOutcome(): String? {
        val line = pendingOutcome
        pendingOutcome = null
        return line
    }

    /** A line the tile itself wants to say (still busy, waiting, …). */
    fun toast(context: Context, line: String) {
        val app = context.applicationContext
        // The caller may be a worker thread; a toast wants the main one.
        Handler(Looper.getMainLooper()).post {
            runCatching { Toast.makeText(app, line, Toast.LENGTH_SHORT).show() }
        }
    }

    private fun outcome(report: String): String? = runCatching {
        val r = JSONObject(report)
        when {
            // An error first: a report can carry both, and "one is already
            // running" is a lie when the check itself never got going.
            r.intArray("errors") > 0 -> "Mail check failed"
            r.optBoolean("skipped") -> "A check is already running"
            else -> when (r.intArray("new")) {
                0 -> "No new mail"
                1 -> "1 new message"
                else -> "${r.intArray("new")} new messages"
            }
        }
    }.getOrNull()

    private fun JSONObject.intArray(name: String): Int = optJSONArray(name)?.length() ?: 0
}
