package de.renier.mailclient.ui.composer

import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.R
import de.renier.mailclient.ui.state.MailState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/**
 * Compose, reply, forward and draft editing: Flutter's composer page, full
 * screen on every width (AGENTS.md: never a dialog on touch). Plain text
 * with Markdown marks, rendered by the core on send; the quoted original
 * rides beside the text box. The account and folder are pinned at open, so
 * switching accounts behind an open composer cannot send as the other one.
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
    val account = remember(accountId) { state.accounts.firstOrNull { it.id == accountId } }
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
    var senderName by remember { mutableStateOf(tf(account?.fromName.orEmpty())) }
    var to by remember { mutableStateOf(tf(seed.to)) }
    var cc by remember { mutableStateOf(tf(seed.cc)) }
    var bcc by remember { mutableStateOf(tf(seed.bcc)) }
    var replyTo by remember { mutableStateOf(tf(seed.replyTo)) }
    var subject by remember { mutableStateOf(tf(seed.subject)) }
    // New mail starts at the top, above the signature.
    var body by remember { mutableStateOf(TextFieldValue(seed.body, TextRange(0))) }
    var showCc by remember { mutableStateOf(seed.cc.isNotEmpty()) }
    var showBcc by remember { mutableStateOf(seed.bcc.isNotEmpty()) }
    var showReplyTo by remember { mutableStateOf(seed.replyTo.isNotEmpty()) }
    var keepQuote by remember { mutableStateOf(true) }
    val picked = remember { mutableStateListOf<PickedFile>() }
    val images = remember { mutableStateMapOf<Int, String>() }
    var nextImage by remember { mutableStateOf(1) }
    var dirty by remember { mutableStateOf(false) }
    var sending by remember { mutableStateOf(false) }
    var savingDraft by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var confirmClose by remember { mutableStateOf(false) }
    var confirmDeleteDraft by remember { mutableStateOf(false) }
    var sendFormat by remember { mutableStateOf("auto") }
    var suggestContacts by remember { mutableStateOf(true) }
    val working = sending || savingDraft

    LaunchedEffect(Unit) {
        val o = withContext(Dispatchers.IO) { runCatching { JSONObject(MailNative.settingsJson()) }.getOrNull() }
        if (o != null) {
            sendFormat = o.optString("compose_send_format", "auto").ifEmpty { "auto" }
            suggestContacts = o.optBoolean("collect_sent_contacts", true)
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
    val quoteShown = keepQuote && seed.quoteHtml.isNotEmpty()
    val imagesJson = JSONObject(images.mapKeys { it.key.toString() }.toMap()).toString()

    fun form(): String {
        val quote = if (quoteShown) seed.quoteHtml else ""
        val html = MailNative.composeBodyHtml(body.text, imagesJson, quote, seed.quoteFirst)
        return JSONObject()
            .put("to", to.text)
            .put("cc", cc.text)
            .put("bcc", bcc.text)
            .put("from", MailNative.effectiveFrom(fromLocal.text, accountEmail))
            .put("from_name", senderName.text)
            .put("reply_to", if (replyToShown) replyTo.text else "")
            .put("subject", subject.text)
            .put("body", body.text)
            .put("body_html", html)
            .put("attachments", JSONArray(picked.map { it.path }))
            .put("draft_uid", seed.draftUid)
            .toString()
    }

    fun send() {
        if (working) return
        if (to.text.isBlank() && cc.text.isBlank() && bcc.text.isBlank()) {
            error = "Add at least one recipient (To, Cc or Bcc)"
            return
        }
        sending = true
        error = null
        // Built here, from the fields as shown; only the queue call goes off
        // the main thread.
        val f = form()
        scope.launch {
            // Validation and queueing run inline in the core: a mistake comes
            // back here with the text intact. Only the SMTP submit is queued;
            // the status strip reports it.
            val failure = withContext(Dispatchers.IO) {
                runCatching { MailNative.sendMail(accountId, folderId, f) }.exceptionOrNull()
            }
            if (failure == null) {
                onClose()
            } else {
                sending = false
                error = failure.message ?: "Could not send"
            }
        }
    }

    fun saveDraft() {
        if (working) return
        savingDraft = true
        error = null
        val f = form()
        scope.launch {
            val failure = withContext(Dispatchers.IO) {
                runCatching { MailNative.saveDraft(accountId, f) }.exceptionOrNull()
            }
            if (failure == null) {
                onClose()
            } else {
                savingDraft = false
                error = failure.message ?: "Could not save the draft"
            }
        }
    }

    fun deleteDraft() {
        scope.launch {
            val failure = withContext(Dispatchers.IO) {
                runCatching { MailNative.deleteDraft(accountId, seed.draftUid) }.exceptionOrNull()
            }
            if (failure == null) onClose() else error = failure.message ?: "Could not delete the draft"
        }
    }

    // Discard asks when there is unsent work, but never touches the server
    // copy of a draft: that is the explicit Delete draft button.
    fun maybeClose() {
        if (working) return
        if (dirty) confirmClose = true else onClose()
    }
    BackHandler { maybeClose() }

    fun insertAtCursor(insert: String) {
        val text = body.text
        val at = body.selection.start.coerceIn(0, text.length)
        val next = text.substring(0, at) + insert + text.substring(at)
        body = TextFieldValue(next, TextRange(at + insert.length))
        dirty = true
    }

    fun applyAction(action: String) {
        val sel = body.selection
        val json = runCatching { MailNative.composeEdit(action, body.text, sel.min, sel.max) }.getOrDefault("")
        if (json.isEmpty()) return
        val e = JSONObject(json)
        body = TextFieldValue(e.optString("text"), TextRange(e.optInt("start"), e.optInt("end")))
        dirty = true
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
            val tokens = mutableListOf<String>()
            for (uri in uris) {
                val result = withContext(Dispatchers.IO) {
                    runCatching {
                        val f = ComposerFiles.copyIn(context, uri) ?: error("The image could not be read")
                        f to MailNative.imageDataUrl(f.path)
                    }
                }
                result.onSuccess { (f, url) ->
                    val id = nextImage++
                    images[id] = url
                    tokens += MailNative.inlineImageToken(id, f.name)
                }.onFailure {
                    // Too large or not an image: say so; attaching still works.
                    state.info(it.message ?: "The image cannot go inline")
                }
            }
            if (tokens.isNotEmpty()) insertAtCursor(tokens.joinToString("\n"))
        }
    }

    Scaffold(
        contentWindowInsets = WindowInsets(0),
        topBar = {
            TopAppBar(
                windowInsets = WindowInsets(0),
                title = { Text(seed.mode.title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    IconButton(onClick = ::maybeClose, enabled = !working) {
                        Icon(painterResource(R.drawable.ic_close), "Close")
                    }
                },
            )
        },
        bottomBar = {
            Surface(color = MaterialTheme.colorScheme.surface) {
                Column {
                    HorizontalDivider()
                    // Qt order, where the thumb is: Delete draft, Discard,
                    // Save draft, Send. A FlowRow, so a narrow phone stacks
                    // the buttons instead of overflowing.
                    FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
                        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                    ) {
                        if (seed.draftUid >= 0) {
                            TextButton(onClick = { confirmDeleteDraft = true }, enabled = !working) {
                                Text("Delete draft")
                            }
                        }
                        TextButton(onClick = ::maybeClose, enabled = !working) { Text("Discard") }
                        OutlinedButton(onClick = ::saveDraft, enabled = !working) {
                            if (savingDraft) CircularProgressIndicator(strokeWidth = 2.dp, modifier = Modifier.size(16.dp))
                            else Text("Save draft")
                        }
                        Button(onClick = ::send, enabled = !working) {
                            if (sending) CircularProgressIndicator(strokeWidth = 2.dp, modifier = Modifier.size(16.dp))
                            else Text("Send")
                        }
                    }
                }
            }
        },
    ) { padding ->
        Column(
            modifier = Modifier
                .padding(padding)
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp, vertical = 8.dp),
        ) {
            // Failures at the top, never scrolled away below a long body.
            error?.let {
                Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(bottom = 8.dp))
            }
            if (seed.replyNotice.isNotEmpty()) ComposerNotice(seed.replyNotice, danger = true)

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

            val quoteCard = @Composable {
                ComposerQuote(seed.quoteHtml, forward = seed.mode == ComposeMode.Forward) {
                    keepQuote = false
                    dirty = true
                }
            }
            if (quoteShown && seed.quoteFirst) quoteCard()
            ComposerEditor(
                value = body,
                onValueChange = { edit(body, it) { v -> body = v } },
                imagesJson = imagesJson,
                sendFormat = sendFormat,
                onAction = ::applyAction,
                onImage = { imagePicker.launch(arrayOf("image/*")) },
                onAttach = { attachPicker.launch(arrayOf("*/*")) },
            )
            if (quoteShown && !seed.quoteFirst) quoteCard()

            if (seed.serverAttachments.isNotEmpty()) {
                ComposerNotice(
                    "${seed.serverAttachments.size} file(s) live on the server copy of this draft. " +
                        "Saving replaces it — re-attach them afterwards.",
                )
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (name in seed.serverAttachments) {
                        AssistChip(onClick = {}, label = { Text(name, maxLines = 1, overflow = TextOverflow.Ellipsis) })
                    }
                }
            }
            AttachmentTray(picked) {
                picked.remove(it)
                dirty = true
            }
        }
    }

    if (confirmClose) {
        AlertDialog(
            onDismissRequest = { confirmClose = false },
            title = { Text("Unsent changes") },
            text = { Text("Discard this message, or keep it as a draft first?") },
            confirmButton = {
                Button(onClick = {
                    confirmClose = false
                    saveDraft()
                }) { Text("Save draft") }
            },
            dismissButton = {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(onClick = { confirmClose = false }) { Text("Cancel") }
                    TextButton(onClick = {
                        confirmClose = false
                        onClose()
                    }) { Text("Discard") }
                }
            },
        )
    }
    if (confirmDeleteDraft) {
        AlertDialog(
            onDismissRequest = { confirmDeleteDraft = false },
            title = { Text("Delete draft?") },
            text = { Text("The server copy is destroyed permanently. This cannot be undone.") },
            confirmButton = {
                Button(
                    onClick = {
                        confirmDeleteDraft = false
                        deleteDraft()
                    },
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.error,
                        contentColor = MaterialTheme.colorScheme.onError,
                    ),
                ) { Text("Delete draft") }
            },
            dismissButton = { TextButton(onClick = { confirmDeleteDraft = false }) { Text("Cancel") } },
        )
    }
}
