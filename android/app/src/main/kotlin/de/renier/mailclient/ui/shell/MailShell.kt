package de.renier.mailclient.ui.shell

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.TextButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.compose.ui.platform.LocalLifecycleOwner
import de.renier.mailclient.MainActivity
import de.renier.mailclient.ReaderActivity
import de.renier.mailclient.ui.accounts.AccountSetupScreen
import de.renier.mailclient.ui.accounts.AccountsScreen
import de.renier.mailclient.ui.folders.FolderManagerScreen
import de.renier.mailclient.ui.folders.FoldersScreen
import de.renier.mailclient.ui.home.HomeScreen
import de.renier.mailclient.ui.list.ListScreen
import de.renier.mailclient.ui.state.MailState

// Step 1 shell: one pane (folders → list, reader is its activity), manual
// back stack (no navigation dependency), system-back support, top bar in the
// Qt toolbar's order, status line with outbox pill, undo snackbar. The dev
// probes live one overflow tap away until their screens land, then go.
private sealed interface Route {
    data object Folders : Route
    data object List : Route
    data object FolderManager : Route
    data object Accounts : Route
    // -1: add; else edit.
    data class Setup(val accountId: Long) : Route
    data object Dev : Route
}

@Composable
fun MailShell(openPayload: String?, onConsumeOpen: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val state = remember { MailState(context.applicationContext, scope) }
    var stack by remember { mutableStateOf(listOf<Route>(Route.Folders)) }
    val route = stack.last()
    val snack = remember { SnackbarHostState() }
    var menu by remember { mutableStateOf(false) }

    fun go(r: Route) {
        stack = (stack + r).takeLast(8)
    }

    fun back() {
        if (stack.size > 1) stack = stack.dropLast(1)
    }

    BackHandler(enabled = stack.size > 1) { back() }

    DisposableEffect(Unit) {
        state.ensureInit()
        onDispose { state.release() }
    }

    // Notification tap: land on the folder, open the message in the reader.
    LaunchedEffect(openPayload) {
        val parts = openPayload?.split(":") ?: return@LaunchedEffect
        if (parts.size == 4 && parts[0] == "mail") {
            val account = parts[1].toLongOrNull()
            val folder = parts[2].toLongOrNull()
            val uid = parts[3].toIntOrNull()
            if (account != null && folder != null && uid != null) {
                state.selectAccount(account)
                state.openFolder(folder)
                stack = listOf(Route.Folders, Route.List)
                context.startActivity(
                    ReaderActivity.openIntent(
                        context,
                        mapOf("accountId" to account, "folderId" to folder, "uid" to uid),
                    ),
                )
            }
        }
        onConsumeOpen()
    }

    // The reader mutates mail behind us: reload on resume when it did.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                if (MainActivity.readerDirty) {
                    MainActivity.readerDirty = false
                    state.refreshFolders(andMessages = true)
                }
                state.markSeen()
            }
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }

    // Undo offers and transient notices surface as snackbars.
    val offer = state.undoOffer
    LaunchedEffect(offer) {
        if (offer != null) {
            val r = snack.showSnackbar(
                message = offer.label.ifEmpty { "Done" },
                actionLabel = "Undo",
                duration = SnackbarDuration.Long,
            )
            if (r == SnackbarResult.ActionPerformed) state.undo()
            state.dismissUndo()
        }
    }
    val notice = state.notice
    LaunchedEffect(notice) {
        if (notice != null) {
            snack.showSnackbar(notice)
            state.consumeNotice()
        }
    }

    Scaffold(
        modifier = Modifier
            .fillMaxSize()
            .safeDrawingPadding(),
        topBar = {
            ShellTopBar(
                title = when (route) {
                    Route.Folders -> state.activeAccount?.email ?: "mailclient"
                    Route.List -> state.openFolder?.leaf ?: "Messages"
                    Route.FolderManager -> "Manage folders"
                    Route.Accounts -> "Accounts"
                    is Route.Setup -> if (route.accountId >= 0) "Edit account" else "Add account"
                    Route.Dev -> "Dev probes"
                },
                canGoBack = stack.size > 1,
                onBack = ::back,
                syncing = state.syncing,
                onSync = { state.syncNow() },
                onCompose = { state.info("Composer arrives in Step 6") },
                onMenu = { menu = true },
            )
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(
                    text = { Text("Manage folders") },
                    onClick = {
                        menu = false
                        if (route != Route.FolderManager) go(Route.FolderManager)
                    },
                )
                DropdownMenuItem(
                    text = { Text("Accounts") },
                    onClick = {
                        menu = false
                        go(Route.Accounts)
                    },
                )
                DropdownMenuItem(
                    text = { Text("Settings") },
                    onClick = {
                        menu = false
                        state.info("Settings arrive in Step 8")
                    },
                )
                DropdownMenuItem(
                    text = { Text("Dev probes") },
                    onClick = {
                        menu = false
                        go(Route.Dev)
                    },
                )
            }
        },
        bottomBar = {
            StatusLine(
                text = state.status,
                error = state.statusError,
                syncing = state.syncing,
                outboxPending = state.outboxPending,
                outboxFailed = state.outboxFailed,
                onOutbox = { state.info("Outbox arrives in Step 9") },
            )
        },
        snackbarHost = { SnackbarHost(snack) },
    ) { padding ->
        Column(modifier = Modifier.padding(padding)) {
            when (route) {
                Route.Folders ->
                    FoldersScreen(
                        state = state,
                        onOpenFolder = { go(Route.List) },
                        onAddAccount = { go(Route.Setup(-1)) },
                    )
                Route.List ->
                    ListScreen(
                        state = state,
                        onOpenReader = { accountId, folderId, uid ->
                            context.startActivity(
                                ReaderActivity.openIntent(
                                    context,
                                    mapOf("accountId" to accountId, "folderId" to folderId, "uid" to uid),
                                ),
                            )
                        },
                    )
                // Jumping to a folder from the manager lands on its list, with
                // the sidebar beneath it for back.
                Route.FolderManager ->
                    FolderManagerScreen(
                        state = state,
                        onOpenFolder = { stack = listOf(Route.Folders, Route.List) },
                    )
                Route.Accounts ->
                    AccountsScreen(
                        state = state,
                        onAdd = { go(Route.Setup(-1)) },
                        onEdit = { go(Route.Setup(it)) },
                    )
                is Route.Setup ->
                    AccountSetupScreen(
                        state = state,
                        accountId = route.accountId,
                        onSaved = { stack = listOf(Route.Folders) },
                        onClose = ::back,
                    )
                Route.Dev -> HomeScreen(openPayload = null, onConsumeOpen = {})
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ShellTopBar(
    title: String,
    canGoBack: Boolean,
    onBack: () -> Unit,
    syncing: Boolean,
    onSync: () -> Unit,
    onCompose: () -> Unit,
    onMenu: () -> Unit,
) {
    TopAppBar(
        title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        navigationIcon = {
            if (canGoBack) {
                TextButton(onClick = onBack) { Text("Back") }
            }
        },
        actions = {
            // Text actions until the Step 4 icon set lands (no new
            // dependency for placeholders — icons come from res/).
            TextButton(onClick = onCompose) { Text("Compose") }
            TextButton(onClick = onSync, enabled = !syncing) {
                Text(if (syncing) "Syncing" else "Sync")
            }
            TextButton(onClick = onMenu) { Text("More") }
        },
    )
}

@Composable
private fun StatusLine(
    text: String,
    error: Boolean,
    syncing: Boolean,
    outboxPending: Int,
    outboxFailed: Boolean,
    onOutbox: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(MaterialTheme.colorScheme.surface)
            .padding(horizontal = 12.dp, vertical = 6.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = (if (syncing) "Syncing… " else "") + text,
            color = if (error || outboxFailed) MaterialTheme.colorScheme.error
            else MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodySmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        if (outboxPending > 0) {
            TextButton(onClick = onOutbox) {
                Text(
                    "Outbox $outboxPending",
                    color = if (outboxFailed) MaterialTheme.colorScheme.error
                    else Color.Unspecified,
                )
            }
        }
    }
}
