package de.renier.mailclient

import android.app.Activity
import android.content.ClipData
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject

// New mail begun outside the app: a tapped mailto: link (VIEW / SENDTO) or
// something shared into it (SEND / SEND_MULTIPLE). It collects what the
// other app handed over into the core's PrefillRequest JSON (the merge, the
// link parsing and the body are mailcore::compose::prefill) and hands it to
// MainActivity in the app's own task, so a share never opens a second shell
// inside the sharing app's task. Shared files go along with their read
// grant; the composer copies them in like picked files.
class ComposeEntryActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        runCatching { forward(intent) }.onFailure { Log.w("mailclient", "compose intent not handled", it) }
        finish()
    }

    private fun forward(source: Intent) {
        val request = JSONObject()
        source.data?.takeIf { it.scheme.equals("mailto", ignoreCase = true) }?.let {
            request.put("mailto", it.toString())
        }
        for ((extra, key) in listOf(Intent.EXTRA_EMAIL to "to", Intent.EXTRA_CC to "cc", Intent.EXTRA_BCC to "bcc")) {
            source.getStringArrayExtra(extra)?.let { request.put(key, JSONArray(it.toList())) }
        }
        source.getStringExtra(Intent.EXTRA_SUBJECT)?.let { request.put("subject", it) }
        source.getCharSequenceExtra(Intent.EXTRA_TEXT)?.let { request.put("text", it.toString()) }

        val files = if (source.action == Intent.ACTION_SEND || source.action == Intent.ACTION_SEND_MULTIPLE) {
            streams(source).filter(::shareable).distinct()
        } else {
            emptyList()
        }
        val payload = JSONObject()
            .put("request", request)
            .put("files", JSONArray(files.map { it.toString() }))
        val open = Intent(this, MainActivity::class.java)
            .setAction(MailNotifier.ACTION_OPEN)
            .putExtra(MailNotifier.EXTRA_PAYLOAD, PREFILL_PREFIX + payload)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        if (files.isNotEmpty()) {
            // Passes this activity's temporary read grant on to MainActivity.
            val clip = ClipData.newRawUri(null, files.first())
            files.drop(1).forEach { clip.addItem(ClipData.Item(it)) }
            open.clipData = clip
            open.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        startActivity(open)
    }

    // EXTRA_STREAM (one Uri or a list), and the ClipData newer apps fill.
    @Suppress("DEPRECATION")
    private fun streams(source: Intent): List<Uri> {
        val out = mutableListOf<Uri>()
        if (source.action == Intent.ACTION_SEND_MULTIPLE) {
            val list = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                source.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                source.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)
            }
            list?.let(out::addAll)
        } else {
            val one = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                source.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
            } else {
                source.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
            }
            one?.let(out::add)
        }
        source.clipData?.let { clip -> for (i in 0 until clip.itemCount) clip.getItemAt(i).uri?.let(out::add) }
        return out
    }

    // Only content:// from another app. A file:// URI or one of our own
    // providers would let any app have us attach our private files (the
    // database, the vault) to a mail. Our own providers always resolve;
    // another app's may not (package visibility), which is fine.
    private fun shareable(uri: Uri): Boolean {
        if (uri.scheme != "content") return false
        val authority = uri.authority ?: return false
        if (authority == packageName || authority.startsWith("$packageName.")) return false
        val owner = runCatching { packageManager.resolveContentProvider(authority, 0)?.packageName }.getOrNull()
        return owner != packageName
    }

    companion object {
        // Open payload: the prefix, then {request: PrefillRequest, files: [uri]}.
        const val PREFILL_PREFIX = "prefill:"
    }
}
