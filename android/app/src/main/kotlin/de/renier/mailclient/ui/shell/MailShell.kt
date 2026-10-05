package de.renier.mailclient.ui.shell

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import de.renier.mailclient.MainActivity
import de.renier.mailclient.R
import de.renier.mailclient.ReaderActivity
import de.renier.mailclient.ui.accounts.AccountSetupScreen
import de.renier.mailclient.ui.accounts.AccountsScreen
import de.renier.mailclient.ui.folders.FolderManagerScreen
import de.renier.mailclient.ui.folders.FoldersScreen
import de.renier.mailclient.ui.home.HomeScreen
import de.renier.mailclient.ui.list.ListScreen
import de.renier.mailclient.ui.state.MailState

// The one-pane shell (folders → list, reader is its activity): manual back
// stack (no navigation dependency), search bar on the mail panes and a plain
// back + title bar on every other page, Compose as the FAB, a status strip
// that only shows up with something to say, undo snackbar. The dev probes
// live in the tools menu until their screens land, then go.
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
    val mailPane = route == Route.Folders || route == Route.List

    fun go(r: Route) {
        stack = (stack + r).takeLast(8)
    }

    fun back() {
        if (stack.size > 1) stack = stack.dropLast(1)
    }

    BackHandler(enabled = stack.size > 1) { back() }
    // Declared later, so it wins: back clears a running search first.
    BackHandler(enabled = mailPane && state.searchQuery.isNotEmpty()) { state.clearSearch() }

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

    // Tools menu: the search scope toggle, then the tools (AGENTS.md shell
    // order). No second Compose or Accounts entry anywhere else.
    val tools = listOf(
        ShellMenuItem(
            label = "Search this folder only",
            icon = R.drawable.ic_search,
            checked = state.searchFolderOnly,
            onClick = { state.toggleSearchScope() },
        ),
        ShellMenuItem("Manage folders", R.drawable.ic_folder_manage) { go(Route.FolderManager) },
        ShellMenuItem("Contacts", R.drawable.ic_contacts) { state.info("Contacts arrive in Step 7") },
        ShellMenuItem("Accounts", R.drawable.ic_person) { go(Route.Accounts) },
        ShellMenuItem("Settings", R.drawable.ic_settings) { state.info("Settings arrive in Step 8") },
        ShellMenuItem("Dev probes", R.drawable.ic_code) { go(Route.Dev) },
    )

    Scaffold(
        modifier = Modifier
            .fillMaxSize()
            .safeDrawingPadding(),
        topBar = {
            Column {
                if (mailPane) {
                    val folderName = state.openFolder?.leaf
                    SearchTopBar(
                        query = state.searchQuery,
                        placeholder = if (state.searchFolderOnly && folderName != null) {
                            "Search in $folderName"
                        } else {
                            "Search mail"
                        },
                        canGoBack = stack.size > 1,
                        onBack = {
                            state.clearSearch()
                            back()
                        },
                        onQuery = {
                            state.setSearch(it)
                            // Results live in the list pane.
                            if (it.isNotEmpty() && route == Route.Folders) go(Route.List)
                        },
                        onClear = { state.clearSearch() },
                        syncing = state.syncing,
                        onSync = { state.syncNow() },
                        menu = tools,
                    )
                } else {
                    PageTopBar(
                        title = when (route) {
                            Route.FolderManager -> "Manage folders"
                            Route.Accounts -> "Accounts"
                            is Route.Setup -> if (route.accountId >= 0) "Edit account" else "Add account"
                            Route.Dev -> "Dev probes"
                            Route.Folders, Route.List -> ""
                        },
                        onBack = ::back,
                    )
                }
                if (state.syncing || state.foldersBusy) {
                    LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                }
            }
        },
        floatingActionButton = {
            if (mailPane && state.accounts.isNotEmpty()) {
                ExtendedFloatingActionButton(
                    onClick = { state.info("Composer arrives in Step 6") },
                    icon = { ShellIcon(R.drawable.ic_edit, null) },
                    text = { Text("Compose") },
                )
            }
        },
        bottomBar = {
            StatusStrip(
                text = state.status,
                error = state.statusError,
                busy = state.syncing || state.foldersBusy,
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
