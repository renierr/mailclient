package de.renier.mailclient.ui.composer

import android.annotation.SuppressLint
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.viewinterop.AndroidView
import de.renier.mailclient.MailNative
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

// A small read-only HTML view for the composer: the Markdown preview and the
// quoted original. The page comes from the core's reader document (its CSP
// blocks the network, theme colours from here), and the WebView itself runs
// without JavaScript, file access or navigation.
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun HtmlPreview(html: String, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val colors = listOf(scheme.surface, scheme.onSurface, scheme.primary, scheme.onSurfaceVariant, scheme.outlineVariant)
        .map { it.toArgb() and 0xFFFFFF }
    var doc by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(html, colors) {
        doc = withContext(Dispatchers.IO) {
            runCatching {
                MailNative.readerDocument(
                    html, "theme", colors[0], colors[1], colors[2], colors[3], colors[4],
                    false, 1f, true,
                )
            }.getOrNull()
        }
    }
    AndroidView(
        modifier = modifier,
        factory = { context ->
            WebView(context).apply {
                settings.apply {
                    javaScriptEnabled = false
                    allowFileAccess = false
                    allowContentAccess = false
                    blockNetworkLoads = true
                }
                webViewClient = object : WebViewClient() {
                    // A preview is not a browser: links stay where they are.
                    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) =
                        !request.url.toString().startsWith("about:")
                }
            }
        },
        update = { web ->
            web.setBackgroundColor(0xFF000000.toInt() or colors[0])
            val d = doc
            if (d != null && web.tag !== d) {
                web.tag = d
                web.loadDataWithBaseURL(null, d, "text/html", "utf-8", null)
            }
        },
        onRelease = { it.destroy() },
    )
}
