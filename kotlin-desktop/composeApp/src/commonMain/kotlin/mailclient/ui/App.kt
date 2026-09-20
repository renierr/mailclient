package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.VerticalDivider
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import mailclient.models.Account
import mailclient.models.Folder
import mailclient.models.MessageDetail
import mailclient.models.MessageRow
import mailclient.models.SearchHit
import mailclient.repo.MailRepository

/**
 * App shell: 3-pane layout (sidebar / list / reader), mirroring
 * crates/mailapp/qml/Main.qml. Selection is a UID, never a row index
 * (QML flaw F1); feeds reload in place after every mutation.
 */
@Composable
fun MailApp(repo: MailRepository) {
    val scope = rememberCoroutineScope()
    var accounts by remember { mutableStateOf(listOf<Account>()) }
    var accountId by remember { mutableStateOf<Long?>(null) }
    var folders by remember { mutableStateOf(listOf<Folder>()) }
    var folderId by remember { mutableStateOf<Long?>(null) }
    var rawRows by remember { mutableStateOf(listOf<MessageRow>()) }
    var currentUid by remember { mutableStateOf<Long?>(null) }
    var detail by remember { mutableStateOf<MessageDetail?>(null) }
    var query by remember { mutableStateOf("") }
    var hits by remember { mutableStateOf(listOf<SearchHit>()) }
    var statusText by remember { mutableStateOf("Ready") }
    var busy by remember { mutableStateOf(false) }
    var unreadTotal by remember { mutableStateOf(0L) }
    var showComposer by remember { mutableStateOf(false) }
    var composerTo by remember { mutableStateOf("") }
    var composerSubject by remember { mutableStateOf("") }
    var composerBody by remember { mutableStateOf("") }
    var showAccounts by remember { mutableStateOf(false) }
    var sidebarVisible by remember { mutableStateOf(true) }

    // Sorting state
    var sortField by remember { mutableStateOf("date") }
    var sortDescending by remember { mutableStateOf(true) }

    suspend fun reloadFolders() {
        val id = accountId ?: return
        folders = withContext(Dispatchers.IO) { repo.folders(id) }
    }

    suspend fun reloadRows() {
        val id = folderId ?: return
        rawRows = withContext(Dispatchers.IO) { repo.messages(id) }
        val uid = currentUid
        if (uid != null && rawRows.none { it.uid == uid }) {
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
        if (uid == currentUid && detail != null) return
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
    val currentFolder = folders.firstOrNull { it.id == folderId }
    val searching = query.length >= 3

    // Filter & Sort
    val filteredRows = if (searching) {
        emptyList()
    } else if (query.isEmpty()) {
        rawRows
    } else {
        rawRows.filter {
            it.subject.contains(query, ignoreCase = true) ||
                it.from.contains(query, ignoreCase = true) ||
                it.snippet.contains(query, ignoreCase = true)
        }
    }

    val sortedRows = remember(filteredRows, sortField, sortDescending) {
        val comparator = when (sortField) {
            "from" -> compareBy<MessageRow> { it.from.lowercase() }
            "subject" -> compareBy<MessageRow> { it.subject.lowercase() }
            else -> compareBy<MessageRow> { it.date }
        }
        if (sortDescending) filteredRows.sortedWith(comparator.reversed())
        else filteredRows.sortedWith(comparator)
    }

    Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        // --- Desktop Toolbar (Height 44dp, integrated search bar matching QML) ---
        Row(
            Modifier
                .fillMaxWidth()
                .height(44.dp)
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .padding(horizontal = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            // Toggle sidebar button (QML ☰)
            Box(
                Modifier
                    .size(30.dp)
                    .clip(RoundedCornerShape(5.dp))
                    .clickable { sidebarVisible = !sidebarVisible }
                    .padding(4.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    "☰",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurface,
                )
            }

            // Compose Button (Primary intent)
            ToolbarButton(
                label = "✎  Compose",
                isPrimary = true,
                onClick = { startCompose() },
            )

            // Search Bar (Centered, flexible width matching available toolbar space)
            Box(
                Modifier
                    .weight(1f, fill = false)
                    .widthIn(min = 160.dp, max = 380.dp)
                    .height(30.dp)
                    .clip(RoundedCornerShape(6.dp))
                    .background(MaterialTheme.colorScheme.surface)
                    .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f), RoundedCornerShape(6.dp))
                    .padding(horizontal = 8.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(6.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("🔍", fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Box(Modifier.weight(1f)) {
                        if (query.isEmpty()) {
                            Text(
                                "Search mail… (3+ letters: all folders)",
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
                            )
                        }
                        BasicTextField(
                            value = query,
                            onValueChange = { runSearch(it) },
                            singleLine = true,
                            textStyle = TextStyle(
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurface,
                            ),
                            cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                            modifier = Modifier.fillMaxWidth(),
                        )
                    }
                    if (query.isNotEmpty()) {
                        Text(
                            "✕",
                            fontSize = 11.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier
                                .clip(CircleShape)
                                .clickable { runSearch("") }
                                .padding(2.dp),
                        )
                    }
                }
            }

            Spacer(Modifier.weight(1f))

            // Sync Button
            ToolbarButton(
                label = if (busy) "⟳ Syncing…" else "⟳ Sync",
                enabled = !busy,
                onClick = { doSync() },
            )

            // Accounts button
            ToolbarButton(
                label = "✉ Accounts",
                onClick = { showAccounts = true },
            )

            if (busy) {
                CircularProgressIndicator(
                    Modifier.size(16.dp),
                    strokeWidth = 2.dp,
                    color = MaterialTheme.colorScheme.primary,
                )
            }

            Text(
                text = statusText,
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
            )

            if (unreadTotal > 0) {
                UnreadPill("$unreadTotal")
            }
        }

        HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.5f))

        // --- 3-Pane Layout with Responsive Sizing ---
        BoxWithConstraints(Modifier.fillMaxSize().weight(1f)) {
            val totalWidth = maxWidth
            val sidebarW = if (totalWidth < 900.dp) 200.dp else 230.dp
            val listW = if (totalWidth < 900.dp) 280.dp else 340.dp

            Row(Modifier.fillMaxSize()) {
                // Sidebar (toggled via ☰)
                if (sidebarVisible) {
                    Sidebar(
                        accounts = accounts,
                        activeAccountId = accountId,
                        onSelectAccount = {
                            accountId = it
                            folderId = null
                            currentUid = null
                            detail = null
                            query = ""
                            hits = emptyList()
                            launchReload("Switching account…")
                        },
                        onManageAccounts = { showAccounts = true },
                        folders = sortedFolders(folders.filter { it.subscribed }),
                        activeFolderId = folderId,
                        onSelectFolder = {
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
                        modifier = Modifier.width(sidebarW).fillMaxHeight(),
                    )
                    VerticalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.5f), modifier = Modifier.fillMaxHeight().width(1.dp))
                }

                // Message List
                MessageListPane(
                    folderName = currentFolder?.displayName() ?: "",
                    rows = sortedRows,
                    hits = if (searching) hits else emptyList(),
                    searching = searching,
                    currentUid = currentUid,
                    currentFolderId = folderId,
                    sortField = sortField,
                    sortDescending = sortDescending,
                    onSortChange = { field, desc ->
                        sortField = field
                        sortDescending = desc
                    },
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
                    modifier = Modifier.width(listW).fillMaxHeight(),
                )

                VerticalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.5f), modifier = Modifier.fillMaxHeight().width(1.dp))

                // Message Reader (Fills remaining width)
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
        }

        HorizontalDivider(color = MaterialTheme.colorScheme.outline.copy(alpha = 0.3f))

        // Status Bar
        Row(
            Modifier
                .fillMaxWidth()
                .height(24.dp)
                .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f))
                .padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                text = activeAccount?.let { "${it.name} <${it.email}>" } ?: "No active account",
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = "In-Process Native Backend (JNI)",
                fontSize = 10.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
            )
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

@Composable
private fun ToolbarButton(
    label: String,
    isPrimary: Boolean = false,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    val bg = if (isPrimary) {
        MaterialTheme.colorScheme.primary
    } else {
        MaterialTheme.colorScheme.surface
    }
    val contentColor = if (isPrimary) {
        Color.White
    } else {
        MaterialTheme.colorScheme.onSurface
    }

    Box(
        Modifier
            .clip(RoundedCornerShape(5.dp))
            .background(if (enabled) bg else bg.copy(alpha = 0.5f))
            .border(
                1.dp,
                if (isPrimary) Color.Transparent else MaterialTheme.colorScheme.outline.copy(alpha = 0.5f),
                RoundedCornerShape(5.dp),
            )
            .clickable(enabled = enabled) { onClick() }
            .padding(horizontal = 10.dp, vertical = 5.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = label,
            fontSize = 12.sp,
            fontWeight = FontWeight.SemiBold,
            color = contentColor,
        )
    }
}
