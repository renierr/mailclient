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
    fun show(context: Context, report: String) {
        toast(context, outcome(report) ?: return)
    }

    /** The check blew up before the core could report anything. */
    fun showFailure(context: Context) {
        toast(context, "Mail check failed")
    }

    private fun toast(context: Context, line: String) {
        val app = context.applicationContext
        // The worker runs on its own thread; a toast wants the main one.
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
