package de.renier.mailclient.ui.composer

import android.annotation.SuppressLint
import android.os.Handler
import android.os.Looper
import android.view.View
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.onFocusedBoundsChanged
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.suspendCancellableCoroutine
import org.json.JSONObject
import org.json.JSONTokener
import kotlin.coroutines.resume
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

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

    /** An image (`data:` URL) at the caret; the sender makes it an inline part. */
    fun insertImage(dataUrl: String) = exec("insertImage", dataUrl)

    internal fun top(px: Int) = run("mc.top($px)")

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

// Calls from the page arrive on the WebView's bridge thread.
private class EditorHost(
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
 * The WYSIWYG body, laid out like the reader (MailWebView): the WebView is
 * the one scroller, [header] (the address fields) overlays the top of the
 * page and rides on its scroll, and the page starts with a spacer of the
 * header's height. A WebView inside a Compose scroll column would keep the
 * drags for itself, so the page could not be scrolled over the body; drags
 * on the header are handed to the page instead.
 *
 * The page is ours (`MailNative.editorDocument`): JavaScript runs because
 * the editor is JavaScript, behind the core's nonce CSP; no network, no
 * files, no navigation. [document] is loaded once per identity — a reload
 * would drop the user's edits.
 */
@OptIn(ExperimentalFoundationApi::class)
@SuppressLint("SetJavaScriptEnabled", "JavascriptInterface")
@Composable
fun ComposerEditor(
    controller: EditorController,
    document: String,
    textZoom: Int,
    onChanged: () -> Unit,
    header: @Composable () -> Unit,
) {
    val density = LocalDensity.current
    val changed by rememberUpdatedState(onChanged)
    var headerPx by remember { mutableIntStateOf(0) }
    var scrollPx by remember { mutableIntStateOf(0) }
    var viewPx by remember { mutableIntStateOf(0) }
    var headerCoords by remember { mutableStateOf<LayoutCoordinates?>(null) }
    var focused by remember { mutableStateOf<Rect?>(null) }
    // Drags and flings on the header move the page under it: on a short
    // screen (landscape, keyboard up) the header can fill the whole view,
    // leaving no body to drag on. The WebView clamps its own scroll.
    val headerScroll = rememberScrollableState { delta ->
        val web = controller.web ?: return@rememberScrollableState 0f
        val before = web.scrollY
        web.scrollBy(0, -delta.roundToInt())
        (before - web.scrollY).toFloat()
    }

    LaunchedEffect(headerPx, controller.ready) {
        controller.top(ceil(headerPx / density.density).toInt())
    }

    // A header field with focus stays on screen, above the keyboard too:
    // the header moves with the page, so scroll the page. The body's
    // min-height leaves room to scroll the whole header away.
    LaunchedEffect(focused, viewPx) {
        val f = focused ?: return@LaunchedEffect
        val web = controller.web ?: return@LaunchedEffect
        val margin = with(density) { 16.dp.toPx() }
        var target = web.scrollY.toFloat()
        target = max(target, f.bottom + margin - viewPx)
        target = min(target, f.top - margin)
        val y = max(0, target.toInt())
        if (y != web.scrollY) web.scrollTo(0, y)
    }

    Box(modifier = Modifier.fillMaxSize().onSizeChanged { viewPx = it.height }) {
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
                            onState = { json ->
                                val o = runCatching { JSONObject(json) }.getOrNull()
                                if (o != null) {
                                    controller.format = FormatState(
                                        bold = o.optBoolean("b"),
                                        italic = o.optBoolean("i"),
                                        underline = o.optBoolean("u"),
                                        list = o.optBoolean("l"),
                                        quote = o.optBoolean("q"),
                                    )
                                }
                            },
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
                    setOnScrollChangeListener { _, _, y, _, _ -> scrollPx = y.coerceAtLeast(0) }
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

        // Once the header is off the top it stays there. Measured at its
        // full height, never squeezed into the view: on a short screen it
        // is taller than the page area and scrolls away like the text.
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .wrapContentHeight(Alignment.Top, unbounded = true)
                .offset { IntOffset(0, -min(scrollPx, headerPx + 1)) }
                .background(MaterialTheme.colorScheme.surface)
                .scrollable(headerScroll, Orientation.Vertical)
                .onSizeChanged { headerPx = it.height }
                .onGloballyPositioned { headerCoords = it }
                .onFocusedBoundsChanged { child ->
                    val h = headerCoords
                    focused = if (child != null && h != null && h.isAttached && child.isAttached) {
                        h.localBoundingBoxOf(child, clipBounds = false)
                    } else {
                        null
                    }
                },
        ) {
            header()
        }
    }
}
