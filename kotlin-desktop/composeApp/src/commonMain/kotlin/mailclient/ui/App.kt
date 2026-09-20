package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.VerticalDivider
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import mailclient.models.Folder
import mailclient.models.MessageDetail
import mailclient.models.MessageRow
import mailclient.repo.MailRepository

/**
 * App shell: 3-pane layout (sidebar / list / reader), mirroring
 * crates/mailapp/qml/Main.qml. Selection is a UID, never a row index
 * (QML flaw F1); feeds reload in place after every mutation.
 */
@Composable
fun MailApp(repo: MailRepository) {
    val scope = rememberCoroutineScope()
    var accounts by remember { mutableStateOf(listOf<mailclient.models.Account>()) }
    var accountId by remember { mutableStateOf<Long?>(null) }
    var folders by remember { mutableStateOf(listOf<Folder>()) }
    var folderId by remember { mutableStateOf<Long?>(null) }
    var rows by remember { mutableStateOf(listOf<MessageRow>()) }
    var currentUid by remember { mutableStateOf<Long?>(null) }
    var detail by remember { mutableStateOf<MessageDetail?>(null) }
    var query by remember { mutableStateOf("") }
    var hits by remember { mutableStateOf(listOf<mailclient.models.SearchHit>()) }
    var statusText by remember { mutableStateOf("Starting…") }
    var busy by remember { mutableStateOf(false) }
    var unreadTotal by remember { mutableStateOf(0L) }
    var showComposer by remember { mutableStateOf(false) }
    var composerTo by remember { mutableStateOf("") }
    var composerSubject by remember { mutableStateOf("") }
    var composerBody by remember { mutableStateOf("") }
    var showAccounts by remember { mutableStateOf(false) }
    var accountMenu by remember { mutableStateOf(false) }

    suspend fun reloadFolders() {
        val id = accountId ?: return
        folders = withContext(Dispatchers.IO) { repo.folders(id) }
    }

    suspend fun reloadRows() {
        val id = folderId ?: return
        rows = withContext(Dispatchers.IO) { repo.messages(id) }
        // Selection survives feed rebuilds by UID; drop it if gone.
        val uid = currentUid
        if (uid != null && rows.none { it.uid == uid }) {
            currentUid = null
            detail = null
        }
    }

    suspend fun refreshStatus() {
        val st = withContext(Dispatchers.IO) { repo.status(accountId) }
        unreadTotal = st.unread
        if (st.recent.isNotEmpty() || st.unread > 0) {
            statusText = if (st.unread == 0L) "No unread mail" else "${st.unread} unread"
        }
    }

    fun startCompose(to: String = "", subject: String = "", body: String = "") {
        composerTo = to
        composerSubject = subject
        composerBody = body
        showComposer = true
    }

    fun doSend(to: String, cc: String, bcc: String, subject: String, body: String) {
        val accId = accountId ?: return
        scope.launch {
            busy = true
            statusText = "Sending mail…"
            try {
                withContext(Dispatchers.IO) {
                    repo.send(
                        accountId = accId,
                        to = to,
                        cc = cc,
                        bcc = bcc,
                        subject = subject,
                        body = body,
                    )
                }
                statusText = "Mail sent"
                showComposer = false
                reloadRows()
                refreshStatus()
            } catch (e: Exception) {
                statusText = "Send failed: ${e.message?.take(160)}"
            } finally {
                busy = false
            }
        }
    }

    fun doDelete(uid: Long) {
        val fid = folderId ?: return
        scope.launch {
            busy = true
            try {
                statusText = "Deleting message…"
                withContext(Dispatchers.IO) { repo.delete(fid, uid) }
                if (currentUid == uid) {
                    currentUid = null
                    detail = null
                }
                reloadRows()
                reloadFolders()
                refreshStatus()
                statusText = "Message deleted"
            } catch (e: Exception) {
                statusText = "Delete failed: ${e.message?.take(160)}"
            } finally {
                busy = false
            }
        }
    }

    fun doArchive(uid: Long) {
        val fid = folderId ?: return
        scope.launch {
            busy = true
            try {
                statusText = "Archiving message…"
                withContext(Dispatchers.IO) { repo.archive(fid, uid) }
                if (currentUid == uid) {
                    currentUid = null
                    detail = null
                }
                reloadRows()
                reloadFolders()
                refreshStatus()
                statusText = "Message archived"
            } catch (e: Exception) {
                statusText = "Archive failed: ${e.message?.take(160)}"
            } finally {
                busy = false
            }
        }
    }

    fun doOpenAttachment(attachmentId: Long) {
        scope.launch {
            busy = true
            statusText = "Downloading attachment…"
            try {
                val path = withContext(Dispatchers.IO) { repo.openAttachment(attachmentId) }
                statusText = "Opened: ${java.io.File(path).name}"
                withContext(Dispatchers.IO) {
                    if (java.awt.Desktop.isDesktopSupported() &&
                        java.awt.Desktop.getDesktop().isSupported(java.awt.Desktop.Action.OPEN)
                    ) {
                        java.awt.Desktop.getDesktop().open(java.io.File(path))
                    }
                }
            } catch (e: Exception) {
                statusText = "Attachment open failed: ${e.message?.take(160)}"
            } finally {
                busy = false
            }
        }
    }

    fun replyMessage(d: MessageDetail, replyAll: Boolean = false) {
        val to = d.reply_to.ifBlank { d.from }
        val subj = if (d.subject.startsWith("Re:", ignoreCase = true)) d.subject else "Re: ${d.subject}"
        val body = "\n\nOn ${d.date}, ${d.from} wrote:\n" + d.readableText().lines().joinToString("\n") { "> $it" }
        startCompose(to = to, subject = subj, body = body)
    }

    fun forwardMessage(d: MessageDetail) {
        val subj = if (d.subject.startsWith("Fwd:", ignoreCase = true)) d.subject else "Fwd: ${d.subject}"
        val body = "\n\n---------- Forwarded message ---------\nFrom: ${d.from}\nDate: ${d.date}\nSubject: ${d.subject}\n\n" + d.readableText()
        startCompose(subject = subj, body = body)
    }

    fun launchReload(status: String = "") {
        scope.launch {
            busy = true
            try {
                if (status.isNotEmpty()) statusText = status
                val accs = withContext(Dispatchers.IO) { repo.accounts() }
                accounts = accs
                if (accountId == null || accs.none { it.id == accountId }) {
                    accountId = accs.firstOrNull()?.id
                }
                reloadFolders()
                val fid = folderId
                val inbox = folders.firstOrNull { it.role == "inbox" } ?: folders.firstOrNull()
                if (fid == null || folders.none { it.id == fid }) {
                    folderId = inbox?.id
                }
                reloadRows()
                refreshStatus()
                if (status.isNotEmpty() && statusText == status) statusText = "Ready"
            } catch (e: Exception) {
                statusText = "Error: ${e.message?.take(160)}"
            } finally {
                busy = false
            }
        }
    }

    fun openMessage(uid: Long) {
        val fid = folderId ?: return
        if (uid == currentUid && detail != null) return // re-click is a no-op (F14)
        scope.launch {
            try {
                val d = withContext(Dispatchers.IO) { repo.message(fid, uid) }
                currentUid = uid
                detail = d
                if (d.unread) {
                    withContext(Dispatchers.IO) { repo.markRead(fid, uid, true) }
                    reloadRows()
                    reloadFolders()
                    refreshStatus()
                }
            } catch (e: Exception) {
                statusText = "Open failed: ${e.message?.take(160)}"
            }
        }
    }

    fun toggleStar(uid: Long, starred: Boolean) {
        val fid = folderId ?: return
        scope.launch {
            try {
                withContext(Dispatchers.IO) { repo.markStar(fid, uid, !starred) }
                reloadRows()
                detail?.let {
                    if (it.uid == uid) {
                        detail = withContext(Dispatchers.IO) { repo.message(fid, uid) }
                    }
                }
            } catch (e: Exception) {
                statusText = "Star failed: ${e.message?.take(160)}"
            }
        }
    }

    fun doSync() {
        scope.launch {
            busy = true
            try {
                statusText = "Syncing…"
                val rep = withContext(Dispatchers.IO) { repo.sync(accountId) }
                if (rep.locked) {
                    statusText = "Sync skipped (locked by another process)"
                } else {
                    val errs = rep.errors + rep.accounts.flatMap { it.errors }
                    statusText = if (errs.isEmpty()) {
                        "Synced — ${rep.fetched} fetched, ${rep.expunged} removed"
                    } else {
                        "Synced with ${errs.size} error(s): ${errs.first().take(140)}"
                    }
                }
                reloadFolders()
                reloadRows()
                refreshStatus()
            } catch (e: Exception) {
                statusText = "Sync failed: ${e.message?.take(160)}"
            } finally {
                busy = false
            }
        }
    }

    fun runSearch(q: String) {
        query = q
        val id = accountId ?: return
        if (q.length < 3) {
            hits = emptyList()
            return
        }
        scope.launch {
            try {
                hits = withContext(Dispatchers.IO) { repo.search(id, q) }
            } catch (e: Exception) {
                statusText = "Search failed: ${e.message?.take(120)}"
            }
        }
    }

    LaunchedEffect(Unit) { launchReload() }

    val activeAccount = accounts.firstOrNull { it.id == accountId }
    val searching = query.length >= 3
    val visibleRows = if (searching) {
        emptyList() // search hits render instead (account-wide FTS, like QML)
    } else if (query.isEmpty()) {
        rows
    } else {
        rows.filter {
            it.subject.contains(query, ignoreCase = true) ||
                it.from.contains(query, ignoreCase = true) ||
                it.snippet.contains(query, ignoreCase = true)
        }
    }

    Column(Modifier.fillMaxSize()) {
        // Toolbar (QML header bar: sync, search, composer, accounts).
        Row(
            Modifier.fillMaxWidth().padding(8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            // Account picker.
            Box {
                OutlinedButton(onClick = { accountMenu = true }) {
                    Text(activeAccount?.email ?: "No account")
                }
                DropdownMenu(expanded = accountMenu, onDismissRequest = { accountMenu = false }) {
                    accounts.forEach { a ->
                        DropdownMenuItem(
                            text = { Text("${a.name} <${a.email}>") },
                            onClick = {
                                accountMenu = false
                                accountId = a.id
                                folderId = null
                                currentUid = null
                                detail = null
                                launchReload("Switching account…")
                            },
                        )
                    }
                }
            }
            Button(onClick = { doSync() }, enabled = !busy) { Text("⟳ Sync") }
            Button(onClick = { startCompose() }) { Text("✎ Compose") }
            OutlinedButton(onClick = { showAccounts = true }) { Text("Accounts") }
            Spacer(Modifier.weight(1f))
            if (busy) CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
            if (unreadTotal > 0) UnreadPill(unreadTotal.toString())
        }
        HorizontalDivider()

        // 3 panes.
        Row(Modifier.fillMaxSize().weight(1f)) {
            Sidebar(
                folders = sortedFolders(folders.filter { it.subscribed }),
                activeId = folderId,
                onSelect = {
                    folderId = it
                    currentUid = null
                    detail = null
                    query = ""
                    hits = emptyList()
                    scope.launch {
                        busy = true
                        try {
                            reloadRows()
                        } finally {
                            busy = false
                        }
                    }
                },
                modifier = Modifier.width(230.dp).fillMaxHeight(),
            )
            VerticalDivider(modifier = Modifier.fillMaxHeight().width(1.dp))
            MessageListPane(
                rows = visibleRows,
                hits = if (searching) hits else emptyList(),
                searching = searching,
                query = query,
                onQuery = { runSearch(it) },
                currentUid = currentUid,
                currentFolderId = folderId,
                onSelect = { openMessage(it) },
                onToggleStar = { uid, starred -> toggleStar(uid, starred) },
                onOpenSearchHit = { hit ->
                    folderId = hit.folder_id
                    scope.launch {
                        try {
                            val d = withContext(Dispatchers.IO) { repo.message(hit.folder_id, hit.uid) }
                            currentUid = hit.uid
                            detail = d
                        } catch (e: Exception) {
                            statusText = "Open failed: ${e.message?.take(120)}"
                        }
                    }
                },
                modifier = Modifier.width(340.dp).fillMaxHeight(),
            )
            VerticalDivider(modifier = Modifier.fillMaxHeight().width(1.dp))
            MessageViewPane(
                detail = detail,
                onReply = { replyMessage(it) },
                onReplyAll = { replyMessage(it, replyAll = true) },
                onForward = { forwardMessage(it) },
                onToggleStar = { uid, starred -> toggleStar(uid, starred) },
                onDelete = { doDelete(it) },
                onArchive = { doArchive(it) },
                onOpenAttachment = { doOpenAttachment(it) },
                modifier = Modifier.weight(1f).fillMaxHeight(),
            )
        }
        HorizontalDivider()
        // Status bar.
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(statusText, fontSize = 12.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }

    if (showComposer) {
        ComposerDialog(
            from = activeAccount?.email ?: "",
            initialTo = composerTo,
            initialSubject = composerSubject,
            initialBody = composerBody,
            onSend = { to, cc, bcc, subject, body ->
                doSend(to, cc, bcc, subject, body)
            },
            onClose = { showComposer = false },
        )
    }
    if (showAccounts) {
        AccountsDialog(
            accounts = accounts,
            activeId = accountId,
            onSelect = {
                accountId = it
                folderId = null
                currentUid = null
                detail = null
                showAccounts = false
                launchReload("Switching account…")
            },
            onClose = { showAccounts = false },
        )
    }
}

/** Rounded unread-count pill (sidebar + toolbar). */
@Composable
fun UnreadPill(text: String) {
    Box(
        Modifier.clip(RoundedCornerShape(10.dp)).background(UnreadAccent).padding(horizontal = 8.dp, vertical = 2.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = androidx.compose.ui.graphics.Color.White, fontSize = 12.sp, fontWeight = FontWeight.Bold)
    }
}
