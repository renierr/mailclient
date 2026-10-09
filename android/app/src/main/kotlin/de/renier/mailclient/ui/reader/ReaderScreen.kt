package de.renier.mailclient.ui.reader

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
import androidx.compose.material3.LocalContentColor
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
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import de.renier.mailclient.MailNative
import de.renier.mailclient.R
import de.renier.mailclient.ui.composer.ComposeMode
import de.renier.mailclient.ui.common.CreateTypedDocument
import de.renier.mailclient.ui.common.MessageDeleteConfirm
import de.renier.mailclient.ui.common.copyToClipboard
import de.renier.mailclient.ui.common.rememberEmlSaver
import de.renier.mailclient.ui.common.writeBytes
import de.renier.mailclient.ui.folders.MoveToDialog
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.afterReaderChange
import de.renier.mailclient.ui.state.deletePrompt
import de.renier.mailclient.ui.state.findSimilar
import de.renier.mailclient.ui.state.offerUndo
import de.renier.mailclient.ui.state.visibleFolders
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
    data class Headers(val headers: JSONObject) : ReaderDialog
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
    // "Edit & resend" on a bounce's report card: the sent original's ids.
    onResend: (Long, Int) -> Unit,
    // "Open sent mail" on a report or read receipt: the original's ids.
    onOpenOriginal: (Long, Int) -> Unit,
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
    // Page width below which the mail's fixed widths are loosened (0: it
    // has none), read off the main thread with the message.
    var fitBelow by remember(folderId, uid) { mutableIntStateOf(0) }
    var headers by remember(folderId, uid) { mutableStateOf(JSONObject()) }
    var error by remember(folderId, uid) { mutableStateOf<String?>(null) }
    var reloadTick by remember { mutableIntStateOf(0) }
    var details by remember(folderId, uid) { mutableStateOf(false) }
    var remoteOnce by remember(folderId, uid) { mutableStateOf(false) }
    var originalColors by remember(folderId, uid) { mutableStateOf(false) }
    var downloadingInline by remember(folderId, uid) { mutableStateOf(false) }
    // Dialog, menu and pending picker bytes belong to this message. The
    // shell keys the reader by message today; the keys keep that true if
    // it ever reuses the composition for the next one.
    var dialog by remember(folderId, uid) { mutableStateOf<ReaderDialog?>(null) }
    var menu by remember(folderId, uid) { mutableStateOf(false) }
    val allowRemote = prefs.loadRemoteImages || remoteOnce

    // Pending bytes for the save pickers (the picker result only has a Uri).
    var pendingSave by remember(folderId, uid) { mutableStateOf<ByteArray?>(null) }
    var pendingSaveAll by remember(folderId, uid) { mutableStateOf<List<SaveFile>>(emptyList()) }

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
                writeBytes(context, uri, bytes)
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
    val emlSaver = rememberEmlSaver(
        onSaved = { state.info("Saved") },
        onFailed = { state.info(it) },
    )

    LaunchedEffect(folderId, uid, reloadTick, allowRemote) {
        try {
            val (m, h, fit) = withContext(Dispatchers.IO) {
                MailNative.ensureInit(context)
                val m = JSONObject(MailNative.readerMessage(folderId, uid))
                // Show-once keeps the remote images the sanitizer would drop.
                if (allowRemote && m.optBoolean("is_html")) {
                    m.put("body_html", MailNative.readerMessageHtml(folderId, uid, true))
                }
                val fit = if (m.optBoolean("is_html")) {
                    runCatching { MailNative.readerFitBelow(m.optString("body_html")).toInt() }.getOrDefault(0)
                } else {
                    0
                }
                Triple(m, JSONObject(MailNative.readerHeaders(folderId, uid)), fit)
            }
            msg = m
            headers = h
            fitBelow = fit
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
    val deletePrompt = remember(folder, prefs.confirmDelete) { state.deletePrompt(bulk = false, listOf(folder)) }
    val deletePermanent = deletePrompt.permanent

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
        // Same staleness as the opener: the stored header may have been
        // corrected by the download that just landed.
        val opener = runCatching { MailNative.attachmentOpenMime(id) }.getOrDefault(mime)
        withContext(Dispatchers.Main) {
            if (bytes == null) {
                state.info("$name is not downloaded yet")
                return@withContext
            }
            pendingSave = bytes
            saveOne.launch(name to opener)
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

    // Always the examine dialog (Copy, and Open where the link is safe),
    // whatever a tap does.
    fun onLongPressUrl(url: String) = bg {
        val info = JSONObject(MailNative.linkInfo(url))
        withContext(Dispatchers.Main) { dialog = ReaderDialog.Link(url, info) }
    }

    LaunchedEffect(error) {
        error?.let {
            state.info(it)
            onClose()
        }
    }

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
                    // Equal thirds: unweighted, the row measured the actions in
                    // turn and squeezed the last one to nothing at 360dp and
                    // 150% text. The labels ellipsize inside their third.
                    Row(
                        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
                        horizontalArrangement = Arrangement.SpaceEvenly,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        val third = Modifier.weight(1f)
                        ReplyAction(R.drawable.ic_reply, "Reply", third) { onCompose(ComposeMode.Reply) }
                        ReplyAction(R.drawable.ic_reply_all, "Reply all", third) { onCompose(ComposeMode.ReplyAll) }
                        ReplyAction(R.drawable.ic_forward, "Forward", third) { onCompose(ComposeMode.Forward) }
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
            val canToggleColors = m.optBoolean("is_html") && m.optBoolean("html_colored") &&
                // In a dark theme the paint itself switches; in a light one
                // only the layout does, so without fixed widths the toggle
                // would visibly do nothing.
                (dark || fitBelow > 0)
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
                    onOpenContact = { openAttachment(it, "text/vcard") },
                    onSaveContact = { id, name -> saveAttachment(id, name, "text/vcard") },
                    onOpenAttachedMail = { openAttachment(it, "message/rfc822") },
                    onSaveAttachedMail = { id, name -> saveAttachment(id, name, "message/rfc822") },
                    onResend = onResend,
                    onOpenOriginal = onOpenOriginal,
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
                            if (deletePrompt.ask) dialog = ReaderDialog.Delete else runDelete()
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
                        // of the fitted ones. Tinted while on, so the state
                        // reads without tapping.
                        if (canToggleColors) {
                            IconButton(onClick = { originalColors = !originalColors }) {
                                Icon(
                                    painterResource(R.drawable.ic_palette),
                                    if (originalColors) {
                                        if (dark) "Darken colours" else "Fit to screen"
                                    } else {
                                        if (dark) "Original colours" else "Original layout"
                                    },
                                    tint = if (originalColors) {
                                        MaterialTheme.colorScheme.primary
                                    } else {
                                        LocalContentColor.current
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
                                    emlSaver(folderId, uid)
                                },
                                onHeaders = {
                                    bg {
                                        val h = JSONObject(MailNative.readerHeaders(folderId, uid))
                                        withContext(Dispatchers.Main) { dialog = ReaderDialog.Headers(h) }
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
                    fitBelow = fitBelow,
                    onTapUrl = ::onTapUrl,
                    onLongPressUrl = ::onLongPressUrl,
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
        ReaderDialog.Delete -> MessageDeleteConfirm(
            subject = m?.optString("subject").orEmpty(),
            count = 1,
            permanent = deletePermanent,
            onConfirm = {
                dialog = null
                runDelete()
            },
            onDismiss = { dialog = null },
        )
        ReaderDialog.Purge -> MessageDeleteConfirm(
            subject = m?.optString("subject").orEmpty(),
            count = 1,
            permanent = true,
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
        is ReaderDialog.Headers -> HeadersDialog(d.headers) { dialog = null }
        is ReaderDialog.Link -> LinkDialog(
            url = d.url,
            info = d.info,
            onCopy = {
                copyToClipboard(context, "link", d.url)
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
private fun ReplyAction(icon: Int, label: String, modifier: Modifier, onClick: () -> Unit) {
    TextButton(onClick = onClick, modifier = modifier, contentPadding = PaddingValues(horizontal = 12.dp)) {
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
        // A blocked link (core `link_info.safe`) can be copied, never opened.
        confirmButton = {
            if (info.optBoolean("safe")) TextButton(onClick = onOpen) { Text("Open in browser") }
        },
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

// Qt's Headers dialog: the main fields as label over value, then the
// complete header block behind a toggle (or why it is missing).
@Composable
private fun HeadersDialog(h: JSONObject, onDismiss: () -> Unit) {
    var showRaw by remember { mutableStateOf(false) }
    val raw = h.optString("raw")
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Headers") },
        text = {
            SelectionContainer {
                Column(
                    modifier = Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    for ((label, key) in listOf(
                        "From" to "from", "To" to "to", "Cc" to "cc", "Date" to "date",
                        "Subject" to "subject", "Message-ID" to "message_id", "Reply-To" to "reply_to",
                    )) {
                        val value = h.optString(key)
                        if (value.isEmpty()) continue
                        Column {
                            Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            Text(value, style = MaterialTheme.typography.bodyMedium)
                        }
                    }
                    TextButton(onClick = { showRaw = !showRaw }, contentPadding = PaddingValues(0.dp)) {
                        Icon(
                            painterResource(if (showRaw) R.drawable.ic_expand_more else R.drawable.ic_chevron_right),
                            null,
                            modifier = Modifier.size(18.dp),
                        )
                        Text("Complete headers", modifier = Modifier.padding(start = 4.dp))
                    }
                    if (showRaw) {
                        Text(
                            raw.ifEmpty { "Complete headers are unavailable until this message is downloaded again." },
                            style = MaterialTheme.typography.bodySmall,
                            fontFamily = if (raw.isEmpty()) null else FontFamily.Monospace,
                        )
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Close") } },
    )
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
