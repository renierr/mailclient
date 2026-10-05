package de.renier.mailclient.ui.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import de.renier.mailclient.JobEvents
import de.renier.mailclient.MainActivity
import de.renier.mailclient.MailNative
import de.renier.mailclient.ReaderActivity
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// Scaffold home: core status, a dev opener for the native reader, and a
// manual background check. The message list, folders and composer arrive
// here screen by screen; until then this is the launcher that proves the
// core, the workers and the reader all run without Flutter.
@Composable
fun HomeScreen(openPayload: String?, onConsumeOpen: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var coreState by remember { mutableStateOf("Starting…") }
    var checkResult by remember { mutableStateOf<String?>(null) }
    var checkBusy by remember { mutableStateOf(false) }
    var resumeTick by remember { mutableStateOf(0) }

    // The native reader mutates mail behind us (readerDirty): reload status
    // whenever we come back, like the lists will.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                if (MainActivity.readerDirty) {
                    MainActivity.readerDirty = false
                    resumeTick++
                }
            }
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }

    fun initCore() {
        scope.launch(Dispatchers.IO) {
            val status = try {
                MailNative.ensureInit(context)
                val plan = MailNative.backgroundPlan()
                "Core ready (resumed $resumeTick×). Background plan: $plan"
            } catch (e: Exception) {
                "Core failed: ${e.message}"
            }
            withContext(Dispatchers.Main) { coreState = status }
        }
    }

    // Re-read status after the reader changed mail. The open payload is
    // a notification tap target for the future message-list screen.
    androidx.compose.runtime.LaunchedEffect(resumeTick) {
        initCore()
        if (openPayload != null) onConsumeOpen()
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .safeDrawingPadding()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("mailclient", style = MaterialTheme.typography.headlineMedium)

        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(12.dp)) {
                Text("Core", style = MaterialTheme.typography.titleSmall)
                Spacer(modifier = Modifier.height(4.dp))
                Text(coreState, style = MaterialTheme.typography.bodySmall)
                Spacer(modifier = Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(onClick = { initCore() }) { Text("Reload status") }
                    if (checkBusy) CircularProgressIndicator()
                }
            }
        }

        ReaderOpener()

        SyncJobsProbe()

        ListBulkProbe()

        ComposerProbe()

        MiscProbe()

        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(12.dp)) {
                Text("Background check", style = MaterialTheme.typography.titleSmall)
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    "Runs the same native check the worker and alarm run.",
                    style = MaterialTheme.typography.bodySmall,
                )
                Spacer(modifier = Modifier.height(8.dp))
                Button(
                    enabled = !checkBusy,
                    onClick = {
                        checkBusy = true
                        scope.launch(Dispatchers.IO) {
                            val report = try {
                                MailNative.ensureInit(context)
                                MailNative.check("manual")
                            } catch (e: Exception) {
                                "failed: ${e.message}"
                            }
                            withContext(Dispatchers.Main) {
                                checkResult = report
                                checkBusy = false
                            }
                        }
                    },
                ) {
                    Text(if (checkBusy) "Checking…" else "Run check now")
                }
                checkResult?.let {
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(it, style = MaterialTheme.typography.bodySmall)
                }
            }
        }

        Card(modifier = Modifier.fillMaxWidth()) {
            Column(modifier = Modifier.padding(12.dp)) {
                Text("Roadmap", style = MaterialTheme.typography.titleSmall)
                Spacer(modifier = Modifier.height(4.dp))
                for (row in roadmap) {
                    Text(
                        (if (row.second) "✓ " else "○ ") + row.first,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
    }
}

// Dev opener for the native reader: ids only (settings ride the defaults),
// like the Flutter experiment channel. Replace with the message list.
@Composable
private fun ReaderOpener() {
    val context = LocalContext.current
    var account by remember { mutableStateOf("") }
    var folder by remember { mutableStateOf("") }
    var uid by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp)) {
            Text("Open reader", style = MaterialTheme.typography.titleSmall)
            Spacer(modifier = Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = account,
                    onValueChange = { account = it },
                    label = { Text("Account") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
                OutlinedTextField(
                    value = folder,
                    onValueChange = { folder = it },
                    label = { Text("Folder") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
                OutlinedTextField(
                    value = uid,
                    onValueChange = { uid = it },
                    label = { Text("UID") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
            }
            error?.let {
                Spacer(modifier = Modifier.height(4.dp))
                Text(it, color = MaterialTheme.colorScheme.error)
            }
            Spacer(modifier = Modifier.height(8.dp))
            Button(onClick = {
                val a = account.toLongOrNull()
                val f = folder.toLongOrNull()
                val u = uid.toIntOrNull()
                if (a == null || f == null || u == null) {
                    error = "Account, folder and UID must all be numbers."
                    return@Button
                }
                error = null
                context.startActivity(
                    ReaderActivity.openIntent(
                        context,
                        mapOf("accountId" to a, "folderId" to f, "uid" to u),
                    ),
                )
            }) {
                Text("Open message")
            }
        }
    }
}

// Step 0b smoke probe: queue sync jobs and watch the finished events come
// back through the JobCallbacks listener. With no accounts yet the jobs
// fail honestly (ok=false events) — the round trip is the proof. Delete
// when the message list owns syncing.
@Composable
private fun SyncJobsProbe() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var account by remember { mutableStateOf("") }
    var folder by remember { mutableStateOf("") }
    var lastEvent by remember { mutableStateOf<String?>(null) }
    var history by remember { mutableStateOf<String?>(null) }
    var error by remember { mutableStateOf<String?>(null) }

    // The listener fires on Rust's net thread; hop to Main for state.
    DisposableEffect(Unit) {
        val sub =
            JobEvents.subscribe(context) { json ->
                scope.launch(Dispatchers.Main) { lastEvent = json.take(300) }
            }
        onDispose { sub.close() }
    }

    fun queue(name: String, call: () -> Unit) {
        error = null
        try {
            MailNative.ensureInit(context)
            call()
        } catch (e: Exception) {
            error = "$name not queued: ${e.message}"
        }
    }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp)) {
            Text("Sync jobs (0b probe)", style = MaterialTheme.typography.titleSmall)
            Spacer(modifier = Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = account,
                    onValueChange = { account = it },
                    label = { Text("Account") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
                OutlinedTextField(
                    value = folder,
                    onValueChange = { folder = it },
                    label = { Text("Folder") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
            }
            Spacer(modifier = Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val a = account.toLongOrNull()
                val f = folder.toLongOrNull()
                TextButton(onClick = {
                    if (a == null) { error = "Account must be a number."; return@TextButton }
                    queue("syncAccount") { MailNative.syncAccount(a) }
                }) { Text("Sync") }
                TextButton(onClick = {
                    if (a == null || f == null) { error = "Account and folder must be numbers."; return@TextButton }
                    queue("syncFolder") { MailNative.syncFolder(a, f) }
                }) { Text("Folder") }
                TextButton(onClick = {
                    if (a == null || f == null) { error = "Account and folder must be numbers."; return@TextButton }
                    queue("loadOlder") { MailNative.loadOlderMessages(a, f) }
                }) { Text("Older") }
                TextButton(onClick = {
                    if (a == null) { error = "Account must be a number."; return@TextButton }
                    queue("refreshFolders") { MailNative.refreshFolders(a) }
                }) { Text("LIST") }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = {
                    queue("markSeen") { MailNative.backgroundMarkSeen() }
                }) { Text("Mark seen") }
                TextButton(onClick = {
                    scope.launch(Dispatchers.IO) {
                        val h = try {
                            MailNative.ensureInit(context)
                            MailNative.backgroundRunHistory().take(300)
                        } catch (e: Exception) {
                            "failed: ${e.message}"
                        }
                        withContext(Dispatchers.Main) { history = h }
                    }
                }) { Text("Run history") }
            }
            error?.let {
                Spacer(modifier = Modifier.height(4.dp))
                Text(it, color = MaterialTheme.colorScheme.error)
            }
            lastEvent?.let {
                Spacer(modifier = Modifier.height(4.dp))
                Text("last event: $it", style = MaterialTheme.typography.bodySmall)
            }
            history?.let {
                Spacer(modifier = Modifier.height(4.dp))
                Text("history: $it", style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

// Step 0c smoke probe: one list page plus the selection-shaped writes,
// moves and folder creation. Delete is undoable (Undo button takes it
// back); purge is not offered here — the list screen will confirm it.
// Delete when the message list lands.
@Composable
private fun ListBulkProbe() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var account by remember { mutableStateOf("") }
    var folder by remember { mutableStateOf("") }
    var uid by remember { mutableStateOf("") }
    var path by remember { mutableStateOf("") }
    var output by remember { mutableStateOf<String?>(null) }
    var lastBatch by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    fun run(name: String, call: () -> String) {
        busy = true
        scope.launch(Dispatchers.IO) {
            val result = try {
                MailNative.ensureInit(context)
                call()
            } catch (e: Exception) {
                "$name failed: ${e.message}"
            }
            withContext(Dispatchers.Main) {
                output = result.take(400)
                busy = false
            }
        }
    }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp)) {
            Text("List + bulk (0c probe)", style = MaterialTheme.typography.titleSmall)
            Spacer(modifier = Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = account,
                    onValueChange = { account = it },
                    label = { Text("Account") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
                OutlinedTextField(
                    value = folder,
                    onValueChange = { folder = it },
                    label = { Text("Folder") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
                OutlinedTextField(
                    value = uid,
                    onValueChange = { uid = it },
                    label = { Text("UID") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
            }
            Spacer(modifier = Modifier.height(8.dp))
            OutlinedTextField(
                value = path,
                onValueChange = { path = it },
                label = { Text("Folder path (create / move destination)") },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val a = account.toLongOrNull()
                val f = folder.toLongOrNull()
                val u = uid.toIntOrNull()
                TextButton(onClick = {
                    if (f == null) { output = "Folder must be a number."; return@TextButton }
                    run("page") { MailNative.messagesJson(f, 20, 0) }
                }) { Text("Page") }
                TextButton(onClick = {
                    if (a == null || f == null || u == null) { output = "Account, folder, UID must be numbers."; return@TextButton }
                    run("read") { MailNative.markReadMany(a, f, "[$u]", true) }
                }) { Text("Read") }
                TextButton(onClick = {
                    if (a == null || f == null || u == null) { output = "Account, folder, UID must be numbers."; return@TextButton }
                    run("star") { MailNative.setStarMany(a, f, "[$u]", true) }
                }) { Text("Star") }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val a = account.toLongOrNull()
                val f = folder.toLongOrNull()
                val u = uid.toIntOrNull()
                TextButton(onClick = {
                    if (a == null || f == null || u == null) { output = "Account, folder, UID must be numbers."; return@TextButton }
                    run("delete") {
                        val r = MailNative.deleteMessages(a, f, "[$u]")
                        lastBatch = Regex(""""batch":"([^"]*)"""").find(r)?.groupValues?.get(1)
                        r
                    }
                }) { Text("Delete") }
                TextButton(onClick = {
                    val b = lastBatch
                    if (b.isNullOrEmpty()) { output = "Nothing to undo yet."; return@TextButton }
                    run("undo") { MailNative.undoMove(b) }
                }) { Text("Undo") }
                TextButton(onClick = {
                    if (a == null || path.isBlank()) { output = "Account must be a number and path non-empty."; return@TextButton }
                    run("create") { MailNative.createFolder(a, path); "create queued" }
                }) { Text("Create") }
            }
            if (busy) {
                Spacer(modifier = Modifier.height(4.dp))
                Text("Working…", style = MaterialTheme.typography.bodySmall)
            }
            output?.let {
                Spacer(modifier = Modifier.height(4.dp))
                Text(it, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

// Step 0d smoke probe: draft shapes (blank/reply/forward/stored), From
// helpers and the send validation path. Live send/save need the account
// password, which the seeded DB does not carry — a queued job failing at
// SMTP with an honest event is the expected proof. Delete when the composer
// lands.
@Composable
private fun ComposerProbe() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var account by remember { mutableStateOf("") }
    var folder by remember { mutableStateOf("") }
    var uid by remember { mutableStateOf("") }
    var text by remember { mutableStateOf("") }
    var output by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    fun run(name: String, call: () -> String) {
        busy = true
        scope.launch(Dispatchers.IO) {
            val result = try {
                MailNative.ensureInit(context)
                call()
            } catch (e: Exception) {
                "$name failed: ${e.message}"
            }
            withContext(Dispatchers.Main) {
                output = result.take(400)
                busy = false
            }
        }
    }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp)) {
            Text("Composer (0d probe)", style = MaterialTheme.typography.titleSmall)
            Spacer(modifier = Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = account,
                    onValueChange = { account = it },
                    label = { Text("Account") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
                OutlinedTextField(
                    value = folder,
                    onValueChange = { folder = it },
                    label = { Text("Folder") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
                OutlinedTextField(
                    value = uid,
                    onValueChange = { uid = it },
                    label = { Text("UID") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
            }
            Spacer(modifier = Modifier.height(8.dp))
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                label = { Text("Address / path (parts, from, inline)") },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val f = folder.toLongOrNull()
                val u = uid.toIntOrNull()
                TextButton(onClick = { run("blank") { MailNative.blankDraft() } }) { Text("Blank") }
                TextButton(onClick = {
                    if (f == null || u == null) { output = "Folder and UID must be numbers."; return@TextButton }
                    run("reply") { MailNative.answerDraft(f, u, "reply") }
                }) { Text("Reply") }
                TextButton(onClick = {
                    if (f == null || u == null) { output = "Folder and UID must be numbers."; return@TextButton }
                    run("forward") { MailNative.answerDraft(f, u, "forward") }
                }) { Text("Forward") }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val a = account.toLongOrNull()
                val u = uid.toIntOrNull()
                TextButton(onClick = {
                    if (a == null || u == null) { output = "Account and UID must be numbers."; return@TextButton }
                    run("draft") { MailNative.draftForm(a, u) }
                }) { Text("Draft") }
                TextButton(onClick = {
                    if (text.isBlank()) { output = "Enter an address first."; return@TextButton }
                    run("parts") { MailNative.senderParts(text) }
                }) { Text("Parts") }
                TextButton(onClick = {
                    if (text.isBlank()) { output = "Enter a local part first."; return@TextButton }
                    run("from") { MailNative.effectiveFrom(text, "me@example.com") }
                }) { Text("From") }
                TextButton(onClick = {
                    if (text.isBlank()) { output = "Enter a file path first."; return@TextButton }
                    run("inline") { MailNative.isInlineImage(text) }
                }) { Text("Inline?") }
            }
            if (busy) {
                Spacer(modifier = Modifier.height(4.dp))
                Text("Working…", style = MaterialTheme.typography.bodySmall)
            }
            output?.let {
                Spacer(modifier = Modifier.height(4.dp))
                Text(it, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

// Step 0e smoke probe: search, contacts, settings, outbox and maintenance
// reads against the seeded DB. Writes here are read-only except the
// queued search backfill (harmless without a server). Delete when the
// settings screen lands.
@Composable
private fun MiscProbe() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var account by remember { mutableStateOf("") }
    var folder by remember { mutableStateOf("") }
    var uid by remember { mutableStateOf("") }
    var query by remember { mutableStateOf("") }
    var output by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    fun run(name: String, call: () -> String) {
        busy = true
        scope.launch(Dispatchers.IO) {
            val result = try {
                MailNative.ensureInit(context)
                call()
            } catch (e: Exception) {
                "$name failed: ${e.message}"
            }
            withContext(Dispatchers.Main) {
                output = result.take(400)
                busy = false
            }
        }
    }

    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(12.dp)) {
            Text("Search + settings (0e probe)", style = MaterialTheme.typography.titleSmall)
            Spacer(modifier = Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = account,
                    onValueChange = { account = it },
                    label = { Text("Account") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
                OutlinedTextField(
                    value = folder,
                    onValueChange = { folder = it },
                    label = { Text("Folder") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
                OutlinedTextField(
                    value = uid,
                    onValueChange = { uid = it },
                    label = { Text("UID") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.weight(1f),
                )
            }
            Spacer(modifier = Modifier.height(8.dp))
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                label = { Text("Query / prefix / segment") },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(modifier = Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val a = account.toLongOrNull()
                val f = folder.toLongOrNull()
                val u = uid.toIntOrNull()
                TextButton(onClick = {
                    if (a == null) { output = "Account must be a number."; return@TextButton }
                    run("search") { MailNative.searchJson(a, query, "") }
                }) { Text("Search") }
                TextButton(onClick = {
                    run("plan") { MailNative.searchPlan(query) }
                }) { Text("Plan") }
                TextButton(onClick = {
                    if (a == null || f == null || u == null) { output = "Account, folder, UID must be numbers."; return@TextButton }
                    run("similar") { MailNative.similarJson(a, f, u) }
                }) { Text("Similar") }
                TextButton(onClick = {
                    run("syntax") { MailNative.searchSyntaxHelp() }
                }) { Text("Syntax") }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val a = account.toLongOrNull()
                TextButton(onClick = {
                    run("contacts") { MailNative.contactsJson(query) }
                }) { Text("Contacts") }
                TextButton(onClick = {
                    run("cleanup") { MailNative.cleanupCandidatesJson() }
                }) { Text("Cleanup") }
                TextButton(onClick = {
                    run("segment") { MailNative.recipientSegment(query) }
                }) { Text("Segment") }
                TextButton(onClick = {
                    if (a == null) { output = "Account must be a number."; return@TextButton }
                    run("outbox") { MailNative.outboxJson(a) }
                }) { Text("Outbox") }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val a = account.toLongOrNull()
                TextButton(onClick = {
                    run("settings") { MailNative.settingsJson() }
                }) { Text("Settings") }
                TextButton(onClick = {
                    run("preset") { MailNative.datePresetRange("week") }
                }) { Text("Preset") }
                TextButton(onClick = {
                    run("paint") { MailNative.readerPaint(false, false, false) }
                }) { Text("Paint") }
                TextButton(onClick = {
                    if (a == null) { output = "Account must be a number."; return@TextButton }
                    run("acct") { MailNative.accountSettingsJson(a) }
                }) { Text("AcctSet") }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = {
                    run("storage") {
                        MailNative.storageStatsJson(
                            context.filesDir.path + "/mailclient.sqlite",
                            context.cacheDir.path,
                        )
                    }
                }) { Text("Storage") }
                TextButton(onClick = {
                    run("bgplan") { MailNative.backgroundPlanJson() }
                }) { Text("BgPlan") }
            }
            if (busy) {
                Spacer(modifier = Modifier.height(4.dp))
                Text("Working…", style = MaterialTheme.typography.bodySmall)
            }
            output?.let {
                Spacer(modifier = Modifier.height(4.dp))
                Text(it, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

private val roadmap = listOf(
    "Native reader (Views, from the experiment)" to true,
    "Background checks, push, notifications (moved, unchanged)" to true,
    "Message list (Compose)" to false,
    "Folder shell + accounts (Compose)" to false,
    "Composer (Compose)" to false,
    "Settings (Compose)" to false,
    "Drop the Flutter embedding" to false,
)
