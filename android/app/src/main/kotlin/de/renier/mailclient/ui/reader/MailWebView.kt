package de.renier.mailclient.ui.reader

import android.annotation.SuppressLint
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.view.View
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.FlingBehavior
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.ScrollScope
import androidx.compose.foundation.gestures.rememberScrollableState
import androidx.compose.foundation.gestures.scrollable
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.wrapContentHeight
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
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.viewinterop.AndroidView
import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlin.math.ceil
import kotlin.math.min

/** What the page is painted with: the core's paint name and palette. */
data class PagePaint(
    val paint: String,
    val paper: Int,
    val ink: Int,
    val link: Int,
    val quote: Int,
    val rule: Int,
)

// Sanitized mail HTML in the system WebView, the Flutter MailWebView's
// design (AGENTS.md reader rule): the WebView keeps its own scroller — one
// sized to a whole newsletter would be one enormous surface — and [header]
// overlays the top of the page, follows its scroll, and the document starts
// with a spacer of the header's height. Drags on the header are handed to
// the WebView, so there is one scroller and one fling.
//
// Belt and braces around the core's sanitizer: JavaScript, file and content
// access off, the core's CSP (no network except allowed remote images), and
// every navigation stopped and handed to [onTapUrl].
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun MailWebView(
    html: String,
    page: PagePaint,
    allowRemote: Boolean,
    textZoom: Int,
    fitWidths: Boolean,
    onTapUrl: (String) -> Unit,
    // Long-press on a link: Qt's right-click Copy / Examine, for touch.
    onLongPressUrl: (String) -> Unit,
    header: @Composable () -> Unit,
) {
    // The page's CSS pixel is the display's own density, not the shell's
    // LocalDensity: that one carries the interface scale, which the WebView
    // never sees, so a spacer sized by it falls short of the header.
    val pagePx = LocalContext.current.resources.displayMetrics.density
    val tap by rememberUpdatedState(onTapUrl)
    val longPress by rememberUpdatedState(onLongPressUrl)
    var headerPx by remember { mutableIntStateOf(0) }
    var scrollPx by remember { mutableIntStateOf(0) }
    var doc by remember { mutableStateOf<String?>(null) }
    val fitBelow = remember(html) { runCatching { MailNative.readerFitBelow(html).toInt() }.getOrDefault(0) }
    val web = remember { WebHandle() }
    // Header drags move the page: px by px while the finger is down (the
    // fraction carried over so slow drags do not stall), then the
    // WebView's own fling so it coasts exactly like a drag on the body.
    // Clamped to the page's own scroll range: an unclamped scrollTo moves
    // the view (and the header with it) on a page that cannot scroll.
    val forward = rememberScrollableState { delta ->
        val view = web.view ?: return@rememberScrollableState 0f
        val want = -delta + web.rest
        val px = want.toInt()
        web.rest = want - px
        val before = view.scrollY
        val target = (before + px).coerceIn(0, view.maxScrollY())
        if (target != before) view.scrollTo(view.scrollX, target)
        if (px != 0 && target == before) {
            web.rest = 0f
            0f
        } else {
            delta
        }
    }
    val fling = remember {
        object : FlingBehavior {
            override suspend fun ScrollScope.performFling(initialVelocity: Float): Float {
                web.view?.flingScroll(0, -initialVelocity.toInt())
                return 0f
            }
        }
    }

    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        val fit = fitWidths && fitBelow > 0 && maxWidth.value < fitBelow
        val topSpace = ceil(headerPx / pagePx).toInt()
        // Rebuilt off the main thread (a mail with inline images is
        // megabytes), and debounced: a reload resets the scroll, so a header
        // still settling (details toggled) must cost one load, not several.
        LaunchedEffect(html, page, allowRemote, topSpace, fit) {
            if (headerPx == 0) return@LaunchedEffect
            delay(if (doc == null) 50 else 150)
            doc = withContext(Dispatchers.IO) {
                MailNative.readerDocumentFull(
                    html, page.paint, page.paper, page.ink, page.link, page.quote, page.rule,
                    allowRemote, topSpace, 1f, fit,
                )
            }
        }

        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                ReaderWebView(context).apply {
                    web.view = this
                    overScrollMode = View.OVER_SCROLL_NEVER
                    settings.apply {
                        javaScriptEnabled = false
                        allowFileAccess = false
                        allowContentAccess = false
                        mediaPlaybackRequiresUserGesture = true
                        setGeolocationEnabled(false)
                        // Honour the core's `width=device-width` viewport;
                        // without these newsletters render as a tiny
                        // ~980px overview.
                        useWideViewPort = true
                        loadWithOverviewMode = true
                        // Pinch zoom (Flutter's WebView has it), without
                        // the old on-screen +/- buttons.
                        setSupportZoom(true)
                        builtInZoomControls = true
                        displayZoomControls = false
                    }
                    webViewClient = object : WebViewClient() {
                        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                            val url = request.url.toString()
                            if (url.startsWith("about:")) return false
                            if (request.isForMainFrame) tap(url)
                            return true
                        }
                    }
                    setOnScrollChangeListener { _, _, y, _, _ -> scrollPx = y.coerceAtLeast(0) }
                    // A text link reports its href directly; a linked image
                    // reports the image, so its href is asked for.
                    setOnLongClickListener { v ->
                        val web = v as WebView
                        val hit = web.hitTestResult
                        when (hit.type) {
                            WebView.HitTestResult.SRC_ANCHOR_TYPE -> {
                                hit.extra?.let { longPress(it) }
                                hit.extra != null
                            }
                            WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE -> {
                                val reply = Handler(Looper.getMainLooper()) { msg ->
                                    msg.data.getString("url")?.takeIf { it.isNotEmpty() }?.let { longPress(it) }
                                    true
                                }.obtainMessage()
                                web.requestFocusNodeHref(reply)
                                true
                            }
                            else -> false
                        }
                    }
                }
            },
            update = { web ->
                web.settings.textZoom = textZoom
                web.setBackgroundColor(0xFF000000.toInt() or page.paper)
                val d = doc
                // Identity, not equality: the document can be megabytes.
                if (d != null && web.tag !== d) {
                    web.tag = d
                    web.loadDataWithBaseURL(null, d, "text/html", "utf-8", null)
                }
            },
            onRelease = {
                if (web.view === it) web.view = null
                it.destroy()
            },
        )

        // Once the header is off the top it stays there: nothing moves while
        // the rest of the mail scrolls. Measured at its full height, never
        // squeezed into a short (landscape) view. A drag that starts on the
        // header (its buttons, a long attachment list) scrolls the page; the
        // drag cancels the tap, as in any scrolling column, so taps still work.
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .wrapContentHeight(Alignment.Top, unbounded = true)
                .offset { IntOffset(0, -min(scrollPx, headerPx + 1)) }
                .scrollable(
                    state = forward,
                    orientation = Orientation.Vertical,
                    // The page draws its own edge; a stretch here would
                    // distort the header alone.
                    overscrollEffect = null,
                    flingBehavior = fling,
                )
                .background(MaterialTheme.colorScheme.surface)
                .onSizeChanged { headerPx = it.height },
        ) {
            header()
        }
    }
}

/** The live WebView for header drags, and the sub-pixel drag left over. */
private class WebHandle {
    var view: ReaderWebView? = null
    var rest = 0f
}

/** A WebView that tells how far its page can scroll (the range is protected). */
private class ReaderWebView(context: Context) : WebView(context) {
    fun maxScrollY(): Int = (computeVerticalScrollRange() - computeVerticalScrollExtent()).coerceAtLeast(0)
}
