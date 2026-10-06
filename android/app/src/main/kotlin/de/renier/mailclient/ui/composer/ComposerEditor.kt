package de.renier.mailclient.ui.composer

import android.annotation.SuppressLint
import android.os.Handler
import android.os.Looper
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.delay
import kotlinx.coroutines.suspendCancellableCoroutine
import org.json.JSONObject
import org.json.JSONTokener
import kotlin.coroutines.resume

/** Formatting at the caret, as the page reports it. */
data class FormatState(
    val bold: Boolean = false,
    val italic: Boolean = false,
    val underline: Boolean = false,
    val list: Boolean = false,
    val quote: Boolean = false,
)

/**
 * The composer's handle on the editor page: calls into its `window.mc`
 * (`mailcore::compose::editor`) and what the page reports back. Calls made
 * before the page has loaded are dropped; nothing can be typed then either.
 */
class EditorController {
    internal var web: WebView? = null
    var ready by mutableStateOf(false)
        internal set
    var format by mutableStateOf(FormatState())
        internal set

    private fun run(js: String) {
        if (ready) web?.evaluateJavascript(js, null)
    }

    private fun q(s: String) = JSONObject.quote(s)

    fun exec(command: String, value: String? = null) =
        run(if (value == null) "mc.exec(${q(command)})" else "mc.exec(${q(command)},${q(value)})")

    fun quote() = run("mc.quote()")

    fun link(url: String) = run("mc.link(${q(url)})")

    /** Keep the selection across a picker or dialog that takes focus away. */
    fun saveSelection() = run("mc.save()")

    /** Formatting at the caret, polled (the push bridge misses caret moves
     * that never become edits — Qt polls `mc.state()` for the same reason).
     * Null while the page is not there. */
    suspend fun stateJson(): String? {
        val w = web
        if (w == null || !ready) return null
        return suspendCancellableCoroutine { c ->
            w.evaluateJavascript("mc.state()") { r ->
                c.resume(runCatching { JSONTokener(r).nextValue() as? String }.getOrNull())
            }
        }
    }

    /** A state JSON from the bridge or the poll. */
    internal fun applyFormat(json: String) {
        val o = runCatching { JSONObject(json) }.getOrNull() ?: return
        format = FormatState(
            bold = o.optBoolean("b"),
            italic = o.optBoolean("i"),
            underline = o.optBoolean("u"),
            list = o.optBoolean("l"),
            quote = o.optBoolean("q"),
        )
    }

    /** An image (`data:` URL) at the caret; the sender makes it an inline part. */
    fun insertImage(dataUrl: String) = exec("insertImage", dataUrl)

    /** The body as HTML, or null while the page is not there. */
    suspend fun html(): String? {
        val w = web
        if (w == null || !ready) return null
        return suspendCancellableCoroutine { c ->
            w.evaluateJavascript("mc.html()") { r ->
                c.resume(runCatching { JSONTokener(r).nextValue() as? String }.getOrNull())
            }
        }
    }
}

// Calls from the page arrive on the WebView's bridge thread. Visible to
// the page's reflection (a private host class is not reliably callable
// through addJavascriptInterface), posted back to the main thread here.
internal class EditorHost(
    private val onChanged: () -> Unit,
    private val onState: (String) -> Unit,
) {
    private val main = Handler(Looper.getMainLooper())

    @JavascriptInterface
    fun changed() {
        main.post(onChanged)
    }

    @JavascriptInterface
    fun state(json: String) {
        main.post { onState(json) }
    }
}

/**
 * The WYSIWYG body: a WebView filling its pane, the one scroller. The
 * address fields stay pinned above it in the screen (unlike the reader,
 * whose header scrolls away with the mail) — a composer that loses its
 * To line mid-draft is a misaddressed mail.
 *
 * The page is ours (`MailNative.editorDocument`): JavaScript runs because
 * the editor is JavaScript, behind the core's nonce CSP; no network, no
 * files, no navigation. [document] is loaded once per identity — a reload
 * would drop the user's edits.
 */
@SuppressLint("SetJavaScriptEnabled", "JavascriptInterface")
@Composable
fun ComposerEditor(
    controller: EditorController,
    document: String,
    textZoom: Int,
    onChanged: () -> Unit,
) {
    val changed by rememberUpdatedState(onChanged)

    // `selectionchange` does not reliably reach the bridge from a WebView
    // (Qt polls `mc.state()` for the same reason), so the toggle state is
    // refreshed on a slow tick while the page is up. The push bridge stays
    // for instant updates on tap; both write the same values.
    LaunchedEffect(controller) {
        while (true) {
            delay(FORMAT_POLL_MS)
            val json = runCatching { controller.stateJson() }.getOrNull() ?: continue
            controller.applyFormat(json)
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                WebView(context).apply {
                    overScrollMode = View.OVER_SCROLL_NEVER
                    settings.apply {
                        javaScriptEnabled = true
                        allowFileAccess = false
                        allowContentAccess = false
                        blockNetworkLoads = true
                        setGeolocationEnabled(false)
                        useWideViewPort = true
                    }
                    addJavascriptInterface(
                        EditorHost(
                            onChanged = { changed() },
                            onState = { json -> controller.applyFormat(json) },
                        ),
                        "MCHost",
                    )
                    webViewClient = object : WebViewClient() {
                        // A tapped link in the draft must not navigate the editor away.
                        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) =
                            !request.url.toString().startsWith("about:")

                        override fun onPageFinished(view: WebView, url: String?) {
                            controller.ready = true
                        }
                    }
                    controller.web = this
                }
            },
            update = { web ->
                web.settings.textZoom = textZoom
                web.setBackgroundColor(android.graphics.Color.TRANSPARENT)
                // Identity, not equality: the document can be megabytes.
                if (web.tag !== document) {
                    web.tag = document
                    controller.ready = false
                    web.loadDataWithBaseURL(null, document, "text/html", "utf-8", null)
                }
            },
            onRelease = {
                if (controller.web === it) {
                    controller.web = null
                    controller.ready = false
                }
                it.destroy()
            },
        )
    }
}

// Slow enough to never matter, fast enough that a caret move lights the
// toolbar up while the finger is still down.
private const val FORMAT_POLL_MS = 500L
