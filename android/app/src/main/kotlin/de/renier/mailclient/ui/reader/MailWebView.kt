package de.renier.mailclient.ui.reader

import android.annotation.SuppressLint
import android.view.View
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.background
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
import androidx.compose.ui.platform.LocalDensity
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
// with a spacer of the header's height. The header takes taps on its
// buttons only; drags anywhere else are the page's own, so there is one
// scroller and one fling.
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
    header: @Composable () -> Unit,
) {
    val density = LocalDensity.current
    val tap by rememberUpdatedState(onTapUrl)
    var headerPx by remember { mutableIntStateOf(0) }
    var scrollPx by remember { mutableIntStateOf(0) }
    var doc by remember { mutableStateOf<String?>(null) }
    val fitBelow = remember(html) { runCatching { MailNative.readerFitBelow(html).toInt() }.getOrDefault(0) }

    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        val fit = fitWidths && fitBelow > 0 && maxWidth.value < fitBelow
        val topSpace = ceil(headerPx / density.density).toInt()
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
                WebView(context).apply {
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
            onRelease = { it.destroy() },
        )

        // Once the header is off the top it stays there: nothing moves while
        // the rest of the mail scrolls. Measured at its full height, never
        // squeezed into a short (landscape) view.
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .wrapContentHeight(Alignment.Top, unbounded = true)
                .offset { IntOffset(0, -min(scrollPx, headerPx + 1)) }
                .background(MaterialTheme.colorScheme.surface)
                .onSizeChanged { headerPx = it.height },
        ) {
            header()
        }
    }
}
