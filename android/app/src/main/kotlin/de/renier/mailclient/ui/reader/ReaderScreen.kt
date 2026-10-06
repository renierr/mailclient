package de.renier.mailclient.ui.reader

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Surface
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import de.renier.mailclient.MailNative
import de.renier.mailclient.R
import de.renier.mailclient.ui.composer.ComposeMode
import de.renier.mailclient.ui.folders.MoveToDialog
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.theme.starColor
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

private sealed interface ReaderDialog {
    data object Delete : ReaderDialog
    data object Purge : ReaderDialog
    data object Move : ReaderDialog
    data class Headers(val text: String) : ReaderDialog
    data class Link(val url: String, val info: JSONObject) : ReaderDialog
}

// The reader as a shell pane (Step 5; replaces the Views ReaderActivity of
// the Flutter experiment). Ids in, everything re-read over JNI. Back and the
// message actions (archive, delete, star, colours, fullscreen, ⋮) are part
// of the scrolling header like Flutter's ReaderHeader, never pinned above
// it; the slim reply strip stays docked at the bottom. Header, cards and
// body scroll as one page. Moving the mail away (delete, archive, move)
// returns to the list with the shell's undo snackbar, like the other
// frontends.
@Composable
fun ReaderScreen(
    state: MailState,
    accountId: Long,
    folderId: Long,
    uid: Int,
    onClose: () -> Unit,
    onShowSimilar: () -> Unit,
    // Reply / Reply all / Forward: the shell loads the core's draft.
    onCompose: (ComposeMode) -> Unit,
    // Beside the list (three panes) the way out closes, not goes back.
    closeIcon: Boolean = false,
    // The reader alone on screen, system bars hidden; the shell owns it.
    fullscreen: Boolean = false,
    onToggleFullscreen: () -> Unit = {},
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val prefs = state.readerPrefs
    val dark = isSystemInDarkTheme()
    val fontScale = LocalConfiguration.current.fontScale

    var msg by remember(folderId, uid) { mutableStateOf<JSONObject?>(null) }
    var headers by remember(folderId, uid) { mutableStateOf(JSONObject()) }
    var error by remember(folderId, uid) { mutableStateOf<String?>(null) }
    var reloadTick by remember { mutableIntStateOf(0) }
    var details by remember(folderId, uid) { mutableStateOf(false) }
    var remoteOnce by remember(folderId, uid) { mutableStateOf(false) }
    var originalColors by remember(folderId, uid) { mutableStateOf(false) }
    var downloadingInline by remember(folderId, uid) { mutableStateOf(false) }
    var dialog by remember { mutableStateOf<ReaderDialog?>(null) }
    var menu by remember { mutableStateOf(false) }
    val allowRemote = prefs.loadRemoteImages || remoteOnce

    // Pending bytes for the save pickers (the picker result only has a Uri).
    var pendingSave by remember { mutableStateOf<ByteArray?>(null) }
    var pendingSaveAll by remember { mutableStateOf<List<SaveFile>>(emptyList()) }

    fun bg(work: suspend () -> Unit) {
        scope.launch(Dispatchers.IO) {
            try {
                work()
            } catch (e: Exception) {
                withContext(Dispatchers.Main) { state.info("Error: ${e.message}") }
            }
        }
    }

    val saveOne = rememberLauncherForActivityResult(CreateTypedDocument()) { uri ->
        val bytes = pendingSave
        pendingSave = null
        if (uri != null && bytes != null) {
            bg {
                ReaderFiles.write(context, uri, bytes)
                withContext(Dispatchers.Main) { state.info("Saved") }
            }
        }
    }
    val saveAll = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { tree ->
        val files = pendingSaveAll
        pendingSaveAll = emptyList()
        if (tree != null && files.isNotEmpty()) {
            bg {
                val n = ReaderFiles.writeAll(context, tree, files)
                withContext(Dispatchers.Main) { state.info("Saved $n file(s)") }
            }
        }
    }

    LaunchedEffect(folderId, uid, reloadTick, allowRemote) {
        try {
            val (m, h) = withContext(Dispatchers.IO) {
                MailNative.ensureInit(context)
                val m = JSONObject(MailNative.readerMessage(folderId, uid))
                // Show-once keeps the remote images the sanitizer would drop.
                if (allowRemote && m.optBoolean("is_html")) {
                    m.put("body_html", MailNative.readerMessageHtml(folderId, uid, true))
                }
                m to JSONObject(MailNative.readerHeaders(folderId, uid))
            }
            msg = m
            headers = h
        } catch (e: Exception) {
            // Gone from the cache (a sync dropped it): back to the list
            // rather than a spinner that never ends.
            error = e.message ?: "Message not available"
        }
    }

    // Mark read on the viewer's terms (core plan); leaving cancels a delay.
    val loaded = msg != null
    LaunchedEffect(folderId, uid, loaded) {
        val m = msg ?: return@LaunchedEffect
        if (!m.optBoolean("unread")) return@LaunchedEffect
        val plan = withContext(Dispatchers.IO) {
            JSONObject(MailNative.markReadPlan(prefs.autoMarkRead, prefs.markReadDelaySecs, true))
        }
        when (plan.optString("plan")) {
            "now" -> Unit
            "after" -> delay(plan.optLong("delay_secs", 0).coerceAtLeast(0) * 1000)
            else -> return@LaunchedEffect
        }
        withContext(Dispatchers.IO) { MailNative.setReadFlag(accountId, folderId, uid, true) }
        state.afterReaderChange()
    }

    val folder = state.folders.firstOrNull { it.id == folderId }
    val deletePermanent = folder?.deleteIsPermanent == true

    fun leaveWith(result: String) {
        val r = runCatching { JSONObject(result) }.getOrNull()
        if (r?.optBoolean("purging") == true) state.info("Deleting…") else state.offerUndo(result)
        state.afterReaderChange()
        onClose()
    }

    fun runDelete() = bg {
        val r = MailNative.deleteMessage(accountId, folderId, uid)
        withContext(Dispatchers.Main) { leaveWith(r) }
    }

    fun openAttachment(id: Long, mime: String) = bg {
        val intent = ReaderFiles.openIntent(state, context, accountId, folderId, uid, id, mime)
        withContext(Dispatchers.Main) {
            runCatching { context.startActivity(intent) }.onFailure { state.info("No app can open this file") }
        }
    }

    fun saveAttachment(id: Long, name: String, mime: String) = bg {
        val bytes = ReaderFiles.ensureBytes(state, accountId, folderId, uid, id)
        withContext(Dispatchers.Main) {
            if (bytes == null) {
                state.info("$name is not downloaded yet")
                return@withContext
            }
            pendingSave = bytes
            saveOne.launch(name to mime)
        }
    }

    fun onTapUrl(url: String) = bg {
        val info = JSONObject(MailNative.linkInfo(url))
        withContext(Dispatchers.Main) {
            when {
                !info.optBoolean("safe") -> state.info("Link blocked")
                prefs.linkClickAction == "browser" -> openBrowser(context, url) { state.info(it) }
                else -> dialog = ReaderDialog.Link(url, info)
            }
        }
    }

    LaunchedEffect(error) { if (error != null) { state.info(error!!); onClose() } }

    val m = msg
    Scaffold(
        contentWindowInsets = WindowInsets(0),
        bottomBar = {
            // A slim strip, not Material's 80dp BottomAppBar: three actions
            // need one 48dp touch row. Docked rather than floating, so it
            // never covers the end of the mail (the HTML body scrolls in
            // its own WebView and cannot scroll out from under an overlay).
            if (m != null) {
                Surface(color = MaterialTheme.colorScheme.surfaceContainer) {
                    Row(
                        modifier = Modifier.fillMaxWidth().height(48.dp),
                        horizontalArrangement = Arrangement.SpaceEvenly,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        ReplyAction(R.drawable.ic_reply, "Reply") { onCompose(ComposeMode.Reply) }
                        ReplyAction(R.drawable.ic_reply_all, "Reply all") { onCompose(ComposeMode.ReplyAll) }
                        ReplyAction(R.drawable.ic_forward, "Forward") { onCompose(ComposeMode.Forward) }
                    }
                }
            }
        },
    ) { padding ->
        Box(modifier = Modifier.fillMaxSize().padding(padding)) {
            if (m == null) {
                // Loading: only the way back; the actions need the message.
                IconButton(onClick = onClose, modifier = Modifier.align(Alignment.TopStart)) {
                    if (closeIcon) {
                        Icon(painterResource(R.drawable.ic_close), "Close")
                    } else {
                        Icon(painterResource(R.drawable.ic_arrow_back), "Back")
                    }
                }
                CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
                return@Box
            }
            val canToggleColors = m.optBoolean("is_html") && m.optBoolean("html_colored")
            val header: @Composable () -> Unit = {
                ReaderHeader(
                    m = m,
                    headers = headers,
                    detailsExpanded = details,
                    onToggleDetails = { details = !details },
                    downloadingInline = downloadingInline,
                    onDownloadInline = {
                        downloadingInline = true
                        bg {
                            ReaderFiles.downloadAll(state, accountId, folderId, uid)
                            withContext(Dispatchers.Main) {
                                downloadingInline = false
                                reloadTick++
                            }
                        }
                    },
                    onOpenAttachment = { openAttachment(it.id, it.mime) },
                    onSaveAttachment = { saveAttachment(it.id, it.fileName, it.mime) },
                    onSaveAll = { files ->
                        bg {
                            val pending = files.mapNotNull { a ->
                                ReaderFiles.ensureBytes(state, accountId, folderId, uid, a.id)?.let {
                                    SaveFile(a.fileName, a.mime, it)
                                }
                            }
                            withContext(Dispatchers.Main) {
                                if (pending.size < files.size) {
                                    state.info("Some files are not downloaded yet")
                                    return@withContext
                                }
                                pendingSaveAll = pending
                                saveAll.launch(null)
                            }
                        }
                    },
                    onOpenEvent = { openAttachment(it, "text/calendar") },
                    onSaveEvent = { id, name -> saveAttachment(id, name, "text/calendar") },
                    onClose = onClose,
                    closeIcon = closeIcon,
                    actions = {
                        IconButton(onClick = {
                            bg {
                                val r = MailNative.archiveMessage(accountId, folderId, uid)
                                withContext(Dispatchers.Main) { leaveWith(r) }
                            }
                        }) { Icon(painterResource(R.drawable.ic_archive), "Archive") }
                        IconButton(onClick = {
                            if (deletePermanent || prefs.confirmDelete) dialog = ReaderDialog.Delete else runDelete()
                        }) { Icon(painterResource(R.drawable.ic_delete), if (deletePermanent) "Delete permanently" else "Delete") }
                        val starred = m.optBoolean("starred")
                        IconButton(onClick = {
                            bg {
                                MailNative.toggleStar(accountId, folderId, uid)
                                withContext(Dispatchers.Main) {
                                    reloadTick++
                                    state.afterReaderChange()
                                }
                            }
                        }) {
                            Icon(
                                painterResource(if (starred) R.drawable.ic_star else R.drawable.ic_star_border),
                                if (starred) "Unstar" else "Star",
                                tint = starColor(starred),
                            )
                        }
                        // "As sent" per message, beside the actions: in a dark
                        // theme the sender's colours instead of the darkened
                        // ones, everywhere the original fixed widths instead
                        // of the fitted ones.
                        if (canToggleColors) {
                            IconButton(onClick = { originalColors = !originalColors }) {
                                Icon(
                                    painterResource(R.drawable.ic_palette),
                                    if (originalColors) {
                                        if (dark) "Darken colours" else "Fit to screen"
                                    } else {
                                        if (dark) "Original colours" else "Original layout"
                                    },
                                )
                            }
                        }
                        IconButton(onClick = onToggleFullscreen) {
                            Icon(
                                painterResource(if (fullscreen) R.drawable.ic_fullscreen_exit else R.drawable.ic_fullscreen),
                                if (fullscreen) "Exit full screen" else "Full screen",
                            )
                        }
                        Box {
                            IconButton(onClick = { menu = true }) { Icon(painterResource(R.drawable.ic_more_vert), "More actions") }
                            ReaderMenu(
                                expanded = menu,
                                onDismiss = { menu = false },
                                showRemote = m.optBoolean("has_remote_images") && !allowRemote,
                                onMove = { dialog = ReaderDialog.Move },
                                onSimilar = {
                                    state.findSimilar(folderId, uid)
                                    onShowSimilar()
                                },
                                onRemote = { remoteOnce = true },
                                onSaveEml = {
                                    bg {
                                        val bytes = MailNative.exportEmlBytes(folderId, uid)
                                        val name = MailNative.suggestedEmlName(folderId, uid)
                                        withContext(Dispatchers.Main) {
                                            pendingSave = bytes
                                            saveOne.launch(name to "message/rfc822")
                                        }
                                    }
                                },
                                onHeaders = {
                                    bg {
                                        val text = headersText(JSONObject(MailNative.readerHeaders(folderId, uid)))
                                        withContext(Dispatchers.Main) { dialog = ReaderDialog.Headers(text) }
                                    }
                                },
                                onPurge = { dialog = ReaderDialog.Purge },
                            )
                        }
                    },
                )
            }
            if (m.optBoolean("is_html")) {
                val scheme = MaterialTheme.colorScheme
                val page = remember(m, dark, originalColors, scheme) {
                    pagePaint(
                        colored = m.optBoolean("html_colored"),
                        dark = dark,
                        keepOriginal = originalColors,
                        theme = intArrayOf(
                            scheme.surface.toArgb(),
                            scheme.onSurface.toArgb(),
                            scheme.primary.toArgb(),
                            scheme.onSurfaceVariant.toArgb(),
                            scheme.outlineVariant.toArgb(),
                        ).map { it and 0xFFFFFF },
                    )
                }
                MailWebView(
                    html = m.optString("body_html"),
                    page = page,
                    allowRemote = allowRemote,
                    // Reader text size x interface scale x system font
                    // size; the shell's density does not reach the page.
                    textZoom = (100 * prefs.scale * state.uiScale * fontScale).toInt(),
                    // "As sent" shows the original fixed widths too.
                    fitWidths = !originalColors,
                    onTapUrl = ::onTapUrl,
                    header = header,
                )
            } else {
                Column(modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                    header()
                    SelectionContainer {
                        Text(
                            m.optString("body_text"),
                            fontSize = (16 * prefs.scale).sp,
                            modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 24.dp),
                        )
                    }
                }
            }
        }
    }

    when (val d = dialog) {
        ReaderDialog.Delete -> ConfirmDialog(
            title = if (deletePermanent) "Delete permanently?" else "Move to Trash?",
            text = if (deletePermanent) "“${m?.optString("subject")}” will be destroyed on the server. This cannot be undone."
            else "“${m?.optString("subject")}” will be moved to Trash.",
            confirm = if (deletePermanent) "Delete permanently" else "Move to Trash",
            onConfirm = { dialog = null; runDelete() },
            onDismiss = { dialog = null },
        )
        ReaderDialog.Purge -> ConfirmDialog(
            title = "Delete permanently?",
            text = "“${m?.optString("subject")}” will be destroyed on the server. This cannot be undone.",
            confirm = "Delete permanently",
            onConfirm = {
                dialog = null
                bg {
                    MailNative.purgeMessage(accountId, folderId, uid)
                    withContext(Dispatchers.Main) {
                        state.info("Deleting…")
                        state.afterReaderChange()
                        onClose()
                    }
                }
            },
            onDismiss = { dialog = null },
        )
        ReaderDialog.Move -> MoveToDialog(
            folders = state.visibleFolders,
            currentFolderId = folderId,
            count = 1,
            subject = m?.optString("subject"),
            onPick = { dest ->
                dialog = null
                bg {
                    val r = MailNative.moveMessage(accountId, folderId, uid, dest.path)
                    withContext(Dispatchers.Main) { leaveWith(r) }
                }
            },
            onDismiss = { dialog = null },
        )
        is ReaderDialog.Headers -> AlertDialog(
            onDismissRequest = { dialog = null },
            title = { Text("Headers") },
            text = {
                SelectionContainer {
                    Text(
                        d.text.ifEmpty { "No headers" },
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState()),
                    )
                }
            },
            confirmButton = { TextButton(onClick = { dialog = null }) { Text("Close") } },
        )
        is ReaderDialog.Link -> LinkDialog(
            url = d.url,
            info = d.info,
            onCopy = {
                (context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager)
                    ?.setPrimaryClip(ClipData.newPlainText("link", d.url))
                dialog = null
                state.info("Link copied")
            },
            onOpen = {
                dialog = null
                openBrowser(context, d.url) { state.info(it) }
            },
            onDismiss = { dialog = null },
        )
        null -> Unit
    }
}

@Composable
private fun ReplyAction(icon: Int, label: String, onClick: () -> Unit) {
    TextButton(onClick = onClick, contentPadding = PaddingValues(horizontal = 12.dp)) {
        Icon(painterResource(icon), null, modifier = Modifier.size(18.dp))
        Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(start = 6.dp))
    }
}

@Composable
private fun ReaderMenu(
    expanded: Boolean,
    onDismiss: () -> Unit,
    showRemote: Boolean,
    onMove: () -> Unit,
    onSimilar: () -> Unit,
    onRemote: () -> Unit,
    onSaveEml: () -> Unit,
    onHeaders: () -> Unit,
    onPurge: () -> Unit,
) {
    @Composable
    fun item(label: String, icon: Int, action: () -> Unit) = DropdownMenuItem(
        text = { Text(label) },
        leadingIcon = { Icon(painterResource(icon), null) },
        onClick = {
            onDismiss()
            action()
        },
    )
    DropdownMenu(expanded = expanded, onDismissRequest = onDismiss) {
        item("Move to…", R.drawable.ic_folder, onMove)
        item("Find similar", R.drawable.ic_search, onSimilar)
        if (showRemote) item("Show remote images", R.drawable.ic_image, onRemote)
        item("Save as .eml…", R.drawable.ic_save, onSaveEml)
        item("Show headers", R.drawable.ic_info, onHeaders)
        item("Delete permanently…", R.drawable.ic_delete_forever, onPurge)
    }
}

@Composable
private fun ConfirmDialog(title: String, text: String, confirm: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(confirm, color = MaterialTheme.colorScheme.error) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun LinkDialog(url: String, info: JSONObject, onCopy: () -> Unit, onOpen: () -> Unit, onDismiss: () -> Unit) {
    fun part(key: String) = info.optString(key).ifEmpty { "—" }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Examine link") },
        text = {
            SelectionContainer {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    for ((label, value) in listOf("Address" to url, "Scheme" to part("scheme"), "Domain" to part("host"), "Path" to part("path"))) {
                        Column {
                            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(value, style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onOpen) { Text("Open in browser") } },
        dismissButton = {
            Row {
                TextButton(onClick = onCopy) { Text("Copy") }
                TextButton(onClick = onDismiss) { Text("Close") }
            }
        },
    )
}

private fun openBrowser(context: Context, url: String, onError: (String) -> Unit) {
    runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url.trim()))) }
        .onFailure { onError("Cannot open link") }
}

private fun headersText(h: JSONObject): String = buildString {
    for ((k, v) in listOf(
        "From" to h.optString("from"),
        "To" to h.optString("to"),
        "Cc" to h.optString("cc"),
        "Date" to h.optString("date"),
        "Subject" to h.optString("subject"),
        "Message-ID" to h.optString("message_id"),
        "Reply-To" to h.optString("reply_to"),
    )) if (v.isNotEmpty()) appendLine("$k: $v")
    val raw = h.optString("raw")
    if (raw.isNotEmpty()) {
        appendLine()
        appendLine("-- Complete headers --")
        append(raw)
    }
}

// The core decides the paint (theme / original / darkened) and the colours
// it writes the page in; only the theme's colours come from here.
private fun pagePaint(colored: Boolean, dark: Boolean, keepOriginal: Boolean, theme: List<Int>): PagePaint {
    val paint = runCatching { MailNative.readerPaint(colored, dark, keepOriginal) }.getOrDefault("theme")
    val p = runCatching { JSONObject(MailNative.readerPalette(paint, theme[0], theme[1], theme[2], theme[3], theme[4])) }
        .getOrNull()
    fun c(key: String, i: Int) = p?.optInt(key, theme[i]) ?: theme[i]
    return PagePaint(paint, c("paper", 0), c("ink", 1), c("link", 2), c("quote", 3), c("rule", 4))
}
