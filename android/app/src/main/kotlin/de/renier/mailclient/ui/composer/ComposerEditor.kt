package de.renier.mailclient.ui.composer

import android.annotation.SuppressLint
import android.os.Handler
import android.os.Looper
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
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
    // The page's content height in device px, tracked so the view can sit
    // inside the screen's scroll instead of scrolling itself.
    var contentPx by mutableStateOf(0)
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

    /** Re-read the page's height; the view is sized to its content so the
     * screen's scroll owns the gesture. Zero while the page is not laid
     * out yet — the caller keeps the old height then. Clamped to
     * [MAX_BODY_PX]: see below. */
    internal fun refreshHeight() {
        val w = web ?: return
        val h = (w.contentHeight * w.resources.displayMetrics.density).toLong()
            .coerceAtMost(MAX_BODY_PX.toLong()).toInt()
        if (h > 0) contentPx = h
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
 * The WYSIWYG body: a WebView sized to its content, one row of the screen's
 * scroll — not a scroller itself. The whole composer page (address fields
 * plus body) scrolls as one, so on a narrow screen, with the keyboard up,
 * the fields scroll off and leave room to type. (The reader instead pins
 * nothing and overlays its header; a composer must keep its fields in the
 * page flow so bring-into-view reaches the focused field.)
 *
 * The height tracks `contentHeight` on every input and poll tick, so the
 * caret — fixed relative to the document top — stays visible without any
 * caret math. [minHeight] fills short screens so a one-line draft does not
 * leave a dead page. Very tall bodies (a long forwarded quote) stop at
 * [MAX_BODY_PX] and scroll inside the view past that — an unbounded height
 * crashes Compose layout (see below), and the reader keeps its own
 * scroller for the same reason.
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
    minHeight: Dp,
    onChanged: () -> Unit,
) {
    val changed by rememberUpdatedState(onChanged)
    val density = LocalDensity.current
    val minPx = with(density) { minHeight.roundToPx() }
    val heightDp = with(density) { controller.contentPx.coerceAtLeast(minPx).coerceAtMost(MAX_BODY_PX).toDp() }

    // `selectionchange` does not reliably reach the bridge from a WebView
    // (Qt polls `mc.state()` for the same reason), so the toggle state is
    // refreshed on a slow tick while the page is up. The push bridge stays
    // for instant updates on tap; both write the same values. The tick also
    // re-measures the height, covering growth no input event reports (an
    // image finishing loading, an undo).
    LaunchedEffect(controller) {
        while (true) {
            delay(FORMAT_POLL_MS)
            val json = runCatching { controller.stateJson() }.getOrNull()
            if (json != null) controller.applyFormat(json)
            controller.refreshHeight()
        }
    }

    AndroidView(
        modifier = Modifier.fillMaxWidth().height(heightDp),
        factory = { context ->
            WebView(context).apply {
                overScrollMode = View.OVER_SCROLL_NEVER
                // No internal range (height == content): drags belong to the
                // screen's scroll through nested scrolling.
                isNestedScrollingEnabled = true
                settings.apply {
                    javaScriptEnabled = true
                    allowFileAccess = false
                    allowContentAccess = false
                    blockNetworkLoads = true
                    setGeolocationEnabled(false)
                    mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
                    useWideViewPort = true
                }
                // `MCHost` is the only bridge into the page. Safe because the
                // document is `mailcore::compose::editor::document` over
                // `sanitize_for_send` (script dropped) with a nonce CSP, and
                // file/network loads are blocked above.
                addJavascriptInterface(
                    EditorHost(
                        onChanged = {
                            controller.refreshHeight()
                            changed()
                        },
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
                        controller.refreshHeight()
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
                controller.contentPx = 0
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

// Slow enough to never matter, fast enough that a caret move lights the
// toolbar up while the finger is still down.
private const val FORMAT_POLL_MS = 500L

// Compose packs both axes of a Constraints into 31 bits
// (`createConstraints`: bits(width) + bits(height) must fit, with a floor
// per axis), so a view around 2^18 px or taller crashes layout with
// "Can't represent a width/height in Constraints" — exactly the forward
// crash: a long quote measured contentHeight × density = 433709 px, and the
// next recomposition (typing in To) laid the page out with it. Past this
// cap the WebView keeps its own scroller instead of growing the page;
// 32_767 px is ~9k dp on a 490 dpi phone, far past any real screen.
private const val MAX_BODY_PX = 32_767
