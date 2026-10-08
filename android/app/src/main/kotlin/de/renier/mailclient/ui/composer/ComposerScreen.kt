package de.renier.mailclient.ui.composer

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AssistChip
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.DeleteConfirmDialog
import de.renier.mailclient.ui.common.FormDialog
import de.renier.mailclient.ui.common.UnsavedChangesDialog
import de.renier.mailclient.ui.common.isShortScreen
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.PendingSend
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/**
 * Compose, reply, forward and draft editing, full screen on every width
 * (AGENTS.md: never a dialog on touch). The body is a WYSIWYG HTML editor
 * like Qt's (ComposerEditor) with an HTML source view; with the send format
 * on auto the core still sends plain text when nothing is formatted. The
 * account and folder are pinned at open, so switching accounts behind an
 * open composer cannot send as the other one.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun ComposerScreen(
    state: MailState,
    seed: ComposerSeed,
    accountId: Long,
    folderId: Long,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // Keyed on the accounts too: they load after the first composition,
    // and a lookup that ran while the list was still empty must re-run.
    val account = remember(accountId, state.accounts) { state.accounts.firstOrNull { it.id == accountId } }
    val accountEmail = account?.email.orEmpty()

    // The account's domain is locked (SPF / DKIM alignment); only the local
    // part edits. A reopened draft keeps its own local part.
    val parts = remember(accountEmail, seed) {
        runCatching {
            JSONObject(MailNative.senderParts(seed.fromAddr.ifEmpty { accountEmail })) to
                JSONObject(MailNative.senderParts(accountEmail))
        }.getOrNull()
    }
    val domain = parts?.second?.optString("domain").orEmpty()

    fun tf(text: String) = TextFieldValue(text, TextRange(text.length))
    var fromLocal by remember { mutableStateOf(tf(parts?.first?.optString("local").orEmpty())) }
    var senderName by remember { mutableStateOf(tf(seed.fromName ?: account?.fromName.orEmpty())) }
    // A cold-start Compose opens before the accounts load: fill the sender
    // fields once they do, unless something was typed there meanwhile.
    LaunchedEffect(parts) {
        val local = parts?.first?.optString("local").orEmpty()
        if (fromLocal.text.isEmpty() && local.isNotEmpty()) fromLocal = tf(local)
    }
    LaunchedEffect(account) {
        val name = account?.fromName.orEmpty()
        if (seed.fromName == null && senderName.text.isEmpty() && name.isNotEmpty()) senderName = tf(name)
    }
    var to by remember { mutableStateOf(tf(seed.to)) }
    var cc by remember { mutableStateOf(tf(seed.cc)) }
    var bcc by remember { mutableStateOf(tf(seed.bcc)) }
    var replyTo by remember { mutableStateOf(tf(seed.replyTo)) }
    var subject by remember { mutableStateOf(tf(seed.subject)) }
    var showCc by remember { mutableStateOf(seed.cc.isNotEmpty()) }
    var showBcc by remember { mutableStateOf(seed.bcc.isNotEmpty()) }
    var showReplyTo by remember { mutableStateOf(seed.replyTo.isNotEmpty()) }
    val picked = remember { mutableStateListOf<PickedFile>().apply { addAll(seed.attachments) } }
    // A send that failed after closing is unsent work again: Discard asks.
    var dirty by remember { mutableStateOf(seed.failure.isNotEmpty()) }
    var sending by remember { mutableStateOf(false) }
    var savingDraft by remember { mutableStateOf(false) }
    var deletingDraft by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf(seed.failure.ifEmpty { null }) }
    var confirmClose by remember { mutableStateOf(false) }
    var confirmDeleteDraft by remember { mutableStateOf(false) }
    var linkDialog by remember { mutableStateOf(false) }
    var sendFormat by remember { mutableStateOf(DEFAULT_SEND_FORMAT) }
    // Receipts this mail asks for: from the settings (compose::Receipts)
    // unless a failed send brought its own back or the user already chose.
    var requestMdn by remember { mutableStateOf(seed.requestMdn ?: false) }
    var requestDsn by remember { mutableStateOf(seed.requestDsn ?: false) }
    var receiptsChosen by remember { mutableStateOf(seed.requestMdn != null) }
    // The core's warning while delivery is on but the server lacked DSN.
    var deliveryNote by remember { mutableStateOf("") }
    var suggestContacts by remember { mutableStateOf(true) }
    val working = sending || savingDraft || deletingDraft
    // Like Qt: nothing to send to, nothing to press. The core checks the
    // addresses themselves when Send runs.
    val hasRecipient = to.text.isNotBlank() || cc.text.isNotBlank() || bcc.text.isNotBlank()

    // The body. The page loads [editorBody] once; leaving the source view
    // starts it again from the edited source. [edits] counts changes for
    // the format note.
    val editor = remember { EditorController() }
    var editorBody by remember { mutableStateOf(seed.bodyHtml) }
    var sourceMode by remember { mutableStateOf(false) }
    var source by remember { mutableStateOf("") }
    var edits by remember { mutableIntStateOf(0) }
    var formatNote by remember { mutableStateOf("") }

    val scheme = MaterialTheme.colorScheme
    // Theme colours are read once per body: a recoloured page would mean a
    // reload, and a reload loses what was typed.
    val document = remember(editorBody) {
        val c = listOf(scheme.surface, scheme.onSurface, scheme.onSurfaceVariant, scheme.primary, scheme.outlineVariant)
            .map { it.toArgb() and 0xFFFFFF }
        MailNative.editorDocument(c[0], c[1], c[2], c[3], c[4], EDITOR_FONT_PX, "Write your message", editorBody)
    }
    val textZoom = (100 * LocalConfiguration.current.fontScale).toInt()

    LaunchedEffect(Unit) {
        val o = withContext(Dispatchers.IO) { runCatching { JSONObject(MailNative.settingsJson()) }.getOrNull() }
        if (o != null) {
            sendFormat = o.optString("compose_send_format", DEFAULT_SEND_FORMAT).ifEmpty { DEFAULT_SEND_FORMAT }
            suggestContacts = o.optBoolean("collect_sent_contacts", true)
        }
        val receipts = withContext(Dispatchers.IO) {
            runCatching { JSONObject(MailNative.receiptDefaults(accountId)) }.getOrNull()
        }
        if (receipts != null) {
            deliveryNote = receipts.optString("delivery_note")
            if (!receiptsChosen) {
                requestMdn = receipts.optBoolean("read")
                requestDsn = receipts.optBoolean("delivery")
            }
        }
    }

    suspend fun currentHtml(): String? = if (sourceMode) source else editor.html()

    // What Send will produce, from the core's rule, a moment after typing.
    LaunchedEffect(edits, sourceMode, sendFormat, editor.ready) {
        delay(FORMAT_NOTE_DELAY_MS)
        val html = currentHtml() ?: return@LaunchedEffect
        formatNote = withContext(Dispatchers.IO) {
            runCatching { MailNative.composeFormatNote(sendFormat, html) }.getOrDefault("")
        }
    }

    // Every edit marks the composer dirty; selection moves alone do not.
    fun edit(old: TextFieldValue, new: TextFieldValue, set: (TextFieldValue) -> Unit) {
        if (new.text != old.text) dirty = true
        set(new)
    }

    // Filled fields always show: hiding one would send addresses nobody sees.
    val ccShown = showCc || cc.text.isNotEmpty()
    val bccShown = showBcc || bcc.text.isNotEmpty()
    val replyToShown = showReplyTo || replyTo.text.isNotEmpty()

    // Qt's payload: the HTML goes as both body and body_html; the core
    // derives the plain text and picks the wire format.
    fun form(html: String): String =
        JSONObject()
            .put("to", to.text)
            .put("cc", cc.text)
            .put("bcc", bcc.text)
            .put("from", MailNative.effectiveFrom(fromLocal.text, accountEmail))
            .put("from_name", senderName.text)
            .put("reply_to", if (replyToShown) replyTo.text else "")
            .put("subject", subject.text)
            .put("body", html)
            .put("body_html", html)
            .put("attachments", JSONArray(picked.map { it.path }))
            .put("draft_uid", seed.draftUid)
            .put("request_mdn", requestMdn)
            .put("request_dsn", requestDsn)
            .toString()

    // Everything as it is about to be sent, for reopening after a late
    // failure (MailState.pendingSend).
    fun snapshot(html: String) = seed.copy(
        fromAddr = MailNative.effectiveFrom(fromLocal.text, accountEmail),
        fromName = senderName.text,
        to = to.text,
        cc = cc.text,
        bcc = bcc.text,
        replyTo = if (replyToShown) replyTo.text else "",
        subject = subject.text,
        bodyHtml = html,
        attachments = picked.toList(),
        failure = "",
        requestMdn = requestMdn,
        requestDsn = requestDsn,
    )

    // Reading the page back is asynchronous, so Send and Save draft finish
    // in a coroutine; the queue call itself runs off the main thread.
    // [before] runs on the main thread just before it, [onFail] after a
    // refusal.
    fun submit(
        failText: String,
        setBusy: (Boolean) -> Unit,
        before: (String) -> Unit = {},
        onFail: () -> Unit = {},
        call: (String) -> Unit,
    ) {
        setBusy(true)
        error = null
        scope.launch {
            val html = currentHtml()
            if (html == null) {
                setBusy(false)
                error = "The editor is still loading"
                return@launch
            }
            val f = form(html)
            before(html)
            val failure = withContext(Dispatchers.IO) { runCatching { call(f) }.exceptionOrNull() }
            if (failure == null) {
                onClose()
            } else {
                onFail()
                setBusy(false)
                error = failure.message ?: failText
            }
        }
    }

    // Validation and queueing run inline in the core: a mistake comes back
    // here with the text intact. Only the SMTP submit is queued; the status
    // strip reports it.
    fun send() {
        if (working || !hasRecipient) return
        submit(
            "Could not send",
            { sending = it },
            before = { state.pendingSend = PendingSend(snapshot(it), accountId, folderId) },
            onFail = { state.pendingSend = null },
        ) { MailNative.sendMail(accountId, folderId, it) }
    }

    fun saveDraft() {
        if (working) return
        submit(
            "Could not save the draft",
            { savingDraft = it },
            before = { state.pendingDraft = PendingSend(snapshot(it), accountId, folderId) },
            onFail = { state.pendingDraft = null },
        ) { MailNative.saveDraft(accountId, it) }
    }

    fun deleteDraft() {
        if (working) return
        deletingDraft = true
        scope.launch {
            val failure = withContext(Dispatchers.IO) {
                runCatching { MailNative.deleteDraft(accountId, seed.draftUid) }.exceptionOrNull()
            }
            if (failure == null) {
                onClose()
            } else {
                deletingDraft = false
                error = failure.message ?: "Could not delete the draft"
            }
        }
    }

    // Discard asks when there is unsent work, but never touches the server
    // copy of a draft: that is the explicit Delete draft button.
    fun maybeClose() {
        if (working) return
        if (dirty) confirmClose = true else onClose()
    }
    BackHandler { maybeClose() }

    // Source view shows exactly what will be sent, and edits round-trip.
    fun toggleSource() {
        if (sourceMode) {
            editorBody = source
            sourceMode = false
        } else {
            scope.launch {
                source = editor.html() ?: editorBody
                sourceMode = true
            }
        }
    }

    val attachPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isEmpty()) return@rememberLauncherForActivityResult
        scope.launch {
            val files = withContext(Dispatchers.IO) { uris.mapNotNull { ComposerFiles.copyIn(context, it) } }
            for (f in files) if (picked.none { it.path == f.path }) picked.add(f)
            if (files.isNotEmpty()) dirty = true
            if (files.size < uris.size) state.info("Some files could not be read")
        }
    }
    val imagePicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isEmpty()) return@rememberLauncherForActivityResult
        scope.launch {
            for (uri in uris) {
                val result = withContext(Dispatchers.IO) {
                    runCatching {
                        val f = ComposerFiles.copyIn(context, uri) ?: error("The image could not be read")
                        MailNative.imageDataUrl(f.path)
                    }
                }
                // Too large or not an image: say so; attaching still works.
                result.onSuccess { editor.insertImage(it) }
                    .onFailure { state.info(it.message ?: "The image cannot go inline") }
            }
        }
    }

    // Every action in one place that the keyboard never covers; Discard is
    // the X (it asks when there is unsaved work).
    val actions: @Composable RowScope.() -> Unit = {
        if (seed.draftUid >= 0) {
            IconButton(onClick = { confirmDeleteDraft = true }, enabled = !working) {
                Icon(painterResource(R.drawable.ic_delete), "Delete draft")
            }
        }
        IconButton(onClick = ::saveDraft, enabled = !working) {
            if (savingDraft) {
                CircularProgressIndicator(strokeWidth = 2.dp, modifier = Modifier.size(20.dp))
            } else {
                Icon(painterResource(R.drawable.ic_save), "Save draft")
            }
        }
        IconButton(onClick = ::send, enabled = !working && hasRecipient) {
            if (sending) {
                CircularProgressIndicator(strokeWidth = 2.dp, modifier = Modifier.size(20.dp))
            } else {
                // An explicit tint would ignore the disabled state.
                Icon(
                    painterResource(R.drawable.ic_send),
                    "Send",
                    tint = if (hasRecipient) MaterialTheme.colorScheme.primary else LocalContentColor.current,
                )
            }
        }
    }
    val closeButton: @Composable () -> Unit = {
        IconButton(onClick = ::maybeClose, enabled = !working) {
            Icon(painterResource(R.drawable.ic_close), "Close")
        }
    }
    val toolbar: @Composable () -> Unit = {
        ComposerToolbar(
            format = editor.format,
            sourceMode = sourceMode,
            onExec = { editor.exec(it) },
            onQuote = { editor.quote() },
            onLink = {
                editor.saveSelection()
                linkDialog = true
            },
            onImage = {
                editor.saveSelection()
                imagePicker.launch(arrayOf("image/*"))
            },
            onAttach = { attachPicker.launch(arrayOf("*/*")) },
            onToggleSource = ::toggleSource,
        )
    }
    // A short screen (phone in landscape) cannot spare a title bar and a
    // formatting bar above the keyboard: one 48dp row holds close, the
    // formatting buttons (scrolling sideways) and the actions.
    val short = isShortScreen()

    Scaffold(
        contentWindowInsets = WindowInsets(0),
        topBar = {
            if (short) {
                Surface(color = MaterialTheme.colorScheme.surface) {
                    Column {
                        Row(
                            verticalAlignment = Alignment.CenterVertically,
                            modifier = Modifier.fillMaxWidth().height(48.dp),
                        ) {
                            closeButton()
                            Box(modifier = Modifier.weight(1f)) { toolbar() }
                            actions()
                        }
                        HorizontalDivider()
                    }
                }
            } else {
                TopAppBar(
                    windowInsets = WindowInsets(0),
                    title = { Text(seed.mode.title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    navigationIcon = closeButton,
                    actions = actions,
                )
            }
        },
        bottomBar = {
            if (!short) {
                Surface(color = MaterialTheme.colorScheme.surface) {
                    Column {
                        HorizontalDivider()
                        toolbar()
                    }
                }
            }
        },
    ) { padding ->
        val header: @Composable () -> Unit = {
            Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                // Failures at the top, never scrolled away below a long body.
                error?.let {
                    Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(bottom = 8.dp))
                }
                // Qt: only while To still holds the Reply-To address.
                if (seed.replyNotice.isNotEmpty() && to.text.trim().equals(seed.replyNoticeAddr, ignoreCase = true)) {
                    ComposerNotice(seed.replyNotice, danger = true)
                }

                ComposerHeaderRow(
                    label = "From",
                    trailing = { ComposerToggle("Reply-To", replyToShown) { showReplyTo = !replyToShown } },
                ) {
                    ComposerTextField(
                        value = senderName,
                        onValueChange = { edit(senderName, it) { v -> senderName = v } },
                        placeholder = account?.name ?: "Your name",
                    )
                    ComposerTextField(
                        value = fromLocal,
                        // The domain is the account's: an `@` here would show an
                        // address that is not the one sent.
                        onValueChange = { edit(fromLocal, it.copy(text = it.text.replace("@", ""))) { v -> fromLocal = v } },
                        placeholder = "address",
                        keyboard = KeyboardType.Email,
                        suffix = domain.ifEmpty { null },
                        alignEnd = true,
                        modifier = Modifier.padding(top = 4.dp),
                    )
                }
                ComposerHeaderRow(
                    label = "To",
                    trailing = {
                        ComposerToggle("Cc", ccShown) { showCc = !ccShown }
                        ComposerToggle("Bcc", bccShown) { showBcc = !bccShown }
                    },
                ) {
                    RecipientField(to, { edit(to, it) { v -> to = v } }, suggestContacts)
                }
                if (ccShown) {
                    ComposerHeaderRow("Cc") { RecipientField(cc, { edit(cc, it) { v -> cc = v } }, suggestContacts) }
                }
                if (bccShown) {
                    ComposerHeaderRow("Bcc") {
                        RecipientField(bcc, { edit(bcc, it) { v -> bcc = v } }, suggestContacts, "Hidden from the other recipients")
                    }
                }
                if (replyToShown) {
                    ComposerHeaderRow("Reply-To") {
                        ComposerTextField(
                            value = replyTo,
                            onValueChange = { edit(replyTo, it) { v -> replyTo = v } },
                            placeholder = "Replies go here instead of From",
                            keyboard = KeyboardType.Email,
                        )
                    }
                }
                ComposerHeaderRow("Subject") {
                    ComposerTextField(subject, { edit(subject, it) { v -> subject = v } })
                }
                // This mail only; a new one starts from the settings again.
                FlowRow(modifier = Modifier.padding(top = 4.dp)) {
                    ComposerToggle("Read receipt", requestMdn) {
                        requestMdn = !requestMdn
                        receiptsChosen = true
                    }
                    ComposerToggle("Delivery confirmation", requestDsn) {
                        requestDsn = !requestDsn
                        receiptsChosen = true
                    }
                }
                if (requestDsn && deliveryNote.isNotEmpty()) ComposerNotice(deliveryNote)

                if (seed.serverAttachments.isNotEmpty()) {
                    ComposerNotice(
                        "${seed.serverAttachments.size} file(s) of this draft could not be downloaded. " +
                            "Saving replaces the server copy without them — re-attach them afterwards.",
                    )
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        for (name in seed.serverAttachments) {
                            AssistChip(onClick = {}, label = { Text(name, maxLines = 1, overflow = TextOverflow.Ellipsis) })
                        }
                    }
                }
                if (seed.filesNotice.isNotEmpty()) {
                    ComposerNotice(seed.filesNotice)
                }
                AttachmentTray(picked) {
                    picked.remove(it)
                    dirty = true
                }
                if (formatNote.isNotEmpty()) {
                    Text(
                        formatNote,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 8.dp),
                    )
                }
            }
            HorizontalDivider()
        }

        BoxWithConstraints(modifier = Modifier.padding(padding).fillMaxSize()) {
            // The whole page is one scroll: fields plus body. On a narrow
            // screen, with the keyboard up, the fields scroll off and leave
            // room to type — a pinned header would eat the viewport.
            val density = LocalDensity.current
            var headerPx by remember { mutableIntStateOf(0) }
            // A one-line draft still fills the view below its fields.
            val minBody = with(density) {
                (maxHeight - headerPx.toDp()).coerceAtLeast(200.dp)
            }
            Column(modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
                if (sourceMode) {
                    header()
                    OutlinedTextField(
                        value = source,
                        onValueChange = {
                            if (it != source) {
                                source = it
                                dirty = true
                                edits++
                            }
                        },
                        placeholder = { Text("HTML source…") },
                        textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                        keyboardOptions = KeyboardOptions(autoCorrectEnabled = false),
                        minLines = 10,
                        modifier = Modifier.fillMaxWidth().padding(16.dp),
                    )
                } else {
                    Box(
                        modifier = Modifier.fillMaxWidth().onSizeChanged { headerPx = it.height },
                    ) {
                        header()
                    }
                    ComposerEditor(
                        controller = editor,
                        document = document,
                        textZoom = textZoom,
                        minHeight = minBody,
                        onChanged = {
                            dirty = true
                            edits++
                        },
                    )
                }
            }
        }
    }

    if (linkDialog) {
        var url by remember { mutableStateOf("") }
        FormDialog(
            title = "Insert link",
            confirmLabel = "Insert",
            onConfirm = {
                linkDialog = false
                if (url.isNotBlank()) editor.link(url.trim())
            },
            onDismiss = { linkDialog = false },
        ) {
            OutlinedTextField(
                value = url,
                onValueChange = { url = it },
                label = { Text("Address") },
                supportingText = { Text("Select text first to turn it into a link.") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
    if (confirmClose) {
        UnsavedChangesDialog(
            title = "Unsent changes",
            text = "Discard this message, or keep it as a draft first?",
            saveLabel = "Save draft",
            onSave = {
                confirmClose = false
                saveDraft()
            },
            onDiscard = {
                confirmClose = false
                onClose()
            },
            onDismiss = { confirmClose = false },
        )
    }
    if (confirmDeleteDraft) {
        DeleteConfirmDialog(
            title = "Delete draft?",
            text = "The server copy is destroyed permanently. This cannot be undone.",
            confirmLabel = "Delete draft",
            onConfirm = {
                confirmDeleteDraft = false
                deleteDraft()
            },
            onDismiss = { confirmDeleteDraft = false },
        )
    }
}

// The editor page's base font size, in CSS px.
private const val EDITOR_FONT_PX = 16

// The send format until the settings load (`compose_send_format`).
private const val DEFAULT_SEND_FORMAT = "auto"

// How long typing pauses before the "sends as" note is recomputed.
private const val FORMAT_NOTE_DELAY_MS = 400L
