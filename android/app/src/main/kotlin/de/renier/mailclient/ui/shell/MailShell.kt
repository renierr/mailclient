package de.renier.mailclient.ui.shell

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import de.renier.mailclient.MailNative
import de.renier.mailclient.MailNotifier
import de.renier.mailclient.R
import de.renier.mailclient.ui.accounts.AccountSetupScreen
import de.renier.mailclient.ui.accounts.AccountsScreen
import de.renier.mailclient.ui.composer.ComposerScreen
import de.renier.mailclient.ui.composer.ComposerSeed
import de.renier.mailclient.ui.contacts.ContactsScreen
import de.renier.mailclient.ui.folders.FolderManagerScreen
import de.renier.mailclient.ui.folders.FoldersScreen
import de.renier.mailclient.ui.list.ListScreen
import de.renier.mailclient.ui.outbox.OutboxScreen
import de.renier.mailclient.ui.reader.ReaderScreen
import de.renier.mailclient.ui.settings.SettingsScreen
import de.renier.mailclient.ui.state.MailState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

// The shell: manual back stack (no navigation dependency), search bar on
// the mail panes and a plain back + title bar on every other page, Compose
// as a bar icon, a status strip that only shows up with something to say,
// undo snackbar.
//
// The mail routes (Folders, List, Reader) lay out by width (paneLayout):
// one pane at a time on a phone, folders + list (the reader taking the
// list's place) or all three side by side on a tablet, with draggable
// dividers like Flutter and Qt. Wide layouts keep no List entry on the
// stack (the list is always showing there), so back walks reader → root.
private sealed interface Route {
    data object Folders : Route
    data object List : Route
    data class Reader(val accountId: Long, val folderId: Long, val uid: Int) : Route
    data object FolderManager : Route
    data object Accounts : Route
    // -1: add; else edit.
    data class Setup(val accountId: Long) : Route
    // Full page on every width; account and folder pinned at open.
    data class Composer(val seed: ComposerSeed, val accountId: Long, val folderId: Long) : Route
    // Draws its own bar (Save); a full page on every width.
    data object Settings : Route
    data object Outbox : Route
    data object Contacts : Route
}

@Composable
fun MailShell(openPayload: String?, onConsumeOpen: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val state = remember { MailState(context.applicationContext, scope) }
    var stack by remember { mutableStateOf(listOf<Route>(Route.Folders)) }
    val route = stack.last()
    val snack = remember { SnackbarHostState() }
    val mailPane = route == Route.Folders || route == Route.List || route is Route.Reader
    val layout = paneLayout(state.uiScale)
    val wide = layout != PaneLayout.One
    val widths = rememberPaneWidths()
    // Reader full screen (Flutter/Qt): no shell bars, no other panes, no
    // system bars. Only meaningful over an open message; leaving it ends it.
    var readerFullscreen by remember { mutableStateOf(false) }
    val onReader = route is Route.Reader
    val fullscreen = readerFullscreen && onReader
    LaunchedEffect(onReader) { if (!onReader) readerFullscreen = false }
    ImmersiveMode(fullscreen)

    fun go(r: Route) {
        stack = (stack + r).takeLast(8)
    }

    fun back() {
        if (stack.size > 1) stack = stack.dropLast(1)
    }

    // The mail stack for this layout, optionally with a message open.
    fun mailStack(reader: Route.Reader? = null): List<Route> =
        (if (wide) listOf(Route.Folders) else listOf(Route.Folders, Route.List)) + listOfNotNull(reader)

    fun openFolderPane() {
        // Wide: the list is beside the folders already; picking a folder
        // (the current one included) closes the reader, like Flutter.
        stack = if (wide) mailStack() else stack + Route.List
    }

    fun openReader(r: Route.Reader) {
        stack = stack.dropLastWhile { it is Route.Reader } + r
    }

    // Seeds read the local database only, but a long thread with inline
    // images is still work: off the main thread, then push the page.
    fun startCompose(load: () -> ComposerSeed) {
        val accountId = state.activeAccountId
        val folderId = state.folderId
        if (accountId < 0) return
        // Qt: a send not yet accepted may still come back to the composer.
        if (state.pendingSend != null) {
            state.info("Still sending the last message…")
            return
        }
        scope.launch {
            val seed = withContext(Dispatchers.IO) {
                runCatching {
                    MailNative.ensureInit(context)
                    load()
                }.getOrNull()
            }
            if (seed == null) {
                state.info("This message is no longer available")
            } else {
                go(Route.Composer(seed, accountId, folderId))
            }
        }
    }

    // A row of the Drafts folder continues the draft instead of reading it.
    fun openRow(accountId: Long, folderId: Long, uid: Int) {
        if (state.folders.firstOrNull { it.id == folderId }?.role == "drafts") {
            startCompose { ComposerSeed.draft(accountId, uid) }
        } else {
            openReader(Route.Reader(accountId, folderId, uid))
        }
    }

    // Rotating or resizing across a breakpoint: drop or restore the List
    // entry so back still walks what is on screen.
    LaunchedEffect(wide) {
        stack = if (wide) {
            stack.filter { it != Route.List }
        } else {
            val i = stack.indexOfFirst { it is Route.Reader }
            if (i > 0 && stack[i - 1] != Route.List) {
                stack.take(i) + Route.List + stack.drop(i)
            } else {
                stack
            }
        }
    }

    BackHandler(enabled = stack.size > 1) { back() }
    // Declared later, so it wins: back clears a running search first —
    // except where the reader covers the list (one/two panes), which back
    // leaves first so a hit opened from search returns to the results.
    val readerCoversList = route is Route.Reader && layout != PaneLayout.Three
    BackHandler(enabled = mailPane && !readerCoversList && state.searchQuery.isNotEmpty()) {
        state.clearSearch()
    }
    // Last, so it wins: back leaves full screen before anything else.
    BackHandler(enabled = fullscreen) { readerFullscreen = false }

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
                stack = mailStack(Route.Reader(account, folder, uid))
            }
        }
        onConsumeOpen()
    }

    // Back in the foreground: tell the background checks what was seen,
    // re-read what is showing (a notification action may have changed it)
    // and sync if it is due. The auto-sync timer only runs while resumed.
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> state.resumed()
                Lifecycle.Event.ON_PAUSE -> state.paused()
                else -> Unit
            }
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }

    // A background check (push, poller, notification button) changed the
    // cache while the app is open: re-read what is showing. The hook fires
    // on whatever thread the check ran on.
    DisposableEffect(Unit) {
        MailNotifier.onMailChanged = {
            scope.launch(Dispatchers.Main) { state.refreshFolders(andMessages = true) }
        }
        onDispose { MailNotifier.onMailChanged = null }
    }

    // Android 13+: ask for the notification permission once something
    // checks in the background, like Flutter at startup. Without it every
    // new-mail notification is dropped silently.
    val notifyPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted -> if (!granted) state.info("Notifications are off: new mail will not alert") }
    LaunchedEffect(state.backgroundChecks) {
        if (state.backgroundChecks &&
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            notifyPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }

    // A send that failed after its composer closed comes back with the text
    // (Qt). New compositions wait for the pending one, so none is open.
    val reopen = state.reopenSend
    LaunchedEffect(reopen) {
        if (reopen != null) {
            state.consumeReopenSend()
            if (stack.last() !is Route.Composer) go(Route.Composer(reopen.seed, reopen.accountId, reopen.folderId))
        }
    }

    // Undo offers and transient notices surface as snackbars. The undo bar
    // lasts exactly the core's grace period (the action is gone after it);
    // a newer offer or Ctrl+Z restarts this effect, which takes the bar down.
    val offer = state.undoOffer
    LaunchedEffect(offer) {
        if (offer != null) {
            val r = withTimeoutOrNull(state.undoGraceSecs * 1000) {
                snack.showSnackbar(
                    message = offer.label.ifEmpty { "Done" },
                    actionLabel = "Undo",
                    duration = SnackbarDuration.Indefinite,
                )
            }
            if (r == SnackbarResult.ActionPerformed && state.undoOffer === offer) state.undo()
            state.dismissUndo(offer)
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
        // Fallback for pull-to-refresh (keyboard, accessibility services).
        ShellMenuItem("Sync now", R.drawable.ic_sync) { state.syncNow() },
        ShellMenuItem("Manage folders", R.drawable.ic_folder_manage) { go(Route.FolderManager) },
        ShellMenuItem("Contacts", R.drawable.ic_contacts) { go(Route.Contacts) },
        ShellMenuItem("Accounts", R.drawable.ic_person) { go(Route.Accounts) },
        ShellMenuItem("Settings", R.drawable.ic_settings) { go(Route.Settings) },
    )

    // Interface scale (Settings): every dp and sp grows with it, like
    // Flutter's and Qt's uiScale.
    val baseDensity = LocalDensity.current
    // Something has to hold focus for hardware keys to arrive here.
    val shellFocus = remember { FocusRequester() }
    LaunchedEffect(route) { runCatching { shellFocus.requestFocus() } }
    CompositionLocalProvider(
        LocalDensity provides Density(baseDensity.density * state.uiScale, baseDensity.fontScale),
    ) {
        Scaffold(
            modifier = Modifier
                .fillMaxSize()
                .safeDrawingPadding()
                // Ctrl+Z on a hardware keyboard takes the undo offer back
                // (Qt/Flutter). A focused text field handles its own first;
                // the composer's editor keeps its own undo.
                .focusRequester(shellFocus)
                .onKeyEvent { e ->
                    val undo = e.type == KeyEventType.KeyDown && e.isCtrlPressed && e.key == Key.Z &&
                        route !is Route.Composer && state.undoOffer != null
                    if (undo) state.undo()
                    undo
                }
                .focusable(),
            topBar = {
                // The one-pane reader draws its own bars (actions need its
                // message); beside other panes it sits under the search bar.
                if (route is Route.Reader && !wide) return@Scaffold
                if (fullscreen) return@Scaffold
                // The composer and Settings draw their own bars.
                if (route is Route.Composer || route == Route.Settings) return@Scaffold
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
                            canGoBack = !wide && stack.size > 1,
                            onBack = {
                                state.clearSearch()
                                back()
                            },
                            onCompose = if (state.accounts.isNotEmpty()) {
                                { startCompose { ComposerSeed.blank() } }
                            } else {
                                null
                            },
                            onQuery = {
                                state.setSearch(it)
                                // Results live in the list pane: bring it forward
                                // where it shares its place.
                                if (it.isNotEmpty()) {
                                    if (!wide && route == Route.Folders) go(Route.List)
                                    if (layout == PaneLayout.Two && route is Route.Reader) stack = mailStack()
                                }
                            },
                            onClear = { state.clearSearch() },
                            menu = tools,
                            onToggleSidebar = if (layout == PaneLayout.Three) {
                                { widths.sidebarVisible = !widths.sidebarVisible }
                            } else {
                                null
                            },
                        )
                    } else {
                        PageTopBar(
                            title = when (route) {
                                Route.FolderManager -> "Manage folders"
                                Route.Accounts -> "Accounts"
                                is Route.Setup -> if (route.accountId >= 0) "Edit account" else "Add account"
                            Route.Outbox -> "Outbox"
                            Route.Contacts -> "Contacts"
                                Route.Folders, Route.List, is Route.Reader, is Route.Composer, Route.Settings -> ""
                            },
                            onBack = ::back,
                        )
                    }
                }
            },
            bottomBar = {
                if (route is Route.Reader && !wide) return@Scaffold
                if (route is Route.Composer || route == Route.Settings || fullscreen) return@Scaffold
                StatusStrip(
                    text = state.status,
                    error = state.statusError,
                    busy = state.busy,
                    outboxPending = state.outboxPending,
                    outboxFailed = state.outboxFailed,
                    outboxLabel = state.outboxLabel,
                    onOutbox = { if (route != Route.Outbox) go(Route.Outbox) },
                    onCopied = state::info,
                )
            },
            snackbarHost = { SnackbarHost(snack) },
        ) { padding ->
            Column(modifier = Modifier.padding(padding)) {
                // The header line: any job in the core's in-flight table, on
                // every page (the reader included), like the desktop's busy
                // indicator. The words go to the status strip below. The slot is
                // always there, so starting a sync never shifts the list.
                Box(modifier = Modifier.fillMaxWidth().height(3.dp)) {
                    if (state.busy) {
                        LinearProgressIndicator(modifier = Modifier.fillMaxSize())
                    }
                }
                val folders: @Composable () -> Unit = {
                    FoldersScreen(
                        state = state,
                        onOpenFolder = ::openFolderPane,
                        onAddAccount = { go(Route.Setup(-1)) },
                    )
                }
                val list: @Composable () -> Unit = {
                    ListScreen(
                        state = state,
                        onOpenReader = ::openRow,
                        openMessage = (route as? Route.Reader)?.let { it.folderId to it.uid },
                    )
                }
                val reader: @Composable (Route.Reader) -> Unit = { r ->
                    // Keyed: opening another message beside the list must not
                    // keep the previous one's loaded state.
                    key(r) {
                        ReaderScreen(
                            state = state,
                            accountId = r.accountId,
                            folderId = r.folderId,
                            uid = r.uid,
                            onClose = ::back,
                            // Similar hits show in the list pane under the reader.
                            onShowSimilar = ::back,
                            onCompose = { mode -> startCompose { ComposerSeed.answer(r.folderId, r.uid, mode) } },
                            closeIcon = layout == PaneLayout.Three,
                            fullscreen = fullscreen,
                            onToggleFullscreen = { readerFullscreen = !readerFullscreen },
                        )
                    }
                }
                if (wide && mailPane) {
                    MailPanes(
                        layout = layout,
                        widths = widths,
                        folders = folders,
                        list = list,
                        reader = (route as? Route.Reader)?.let { r -> { reader(r) } },
                        fullscreen = fullscreen,
                    )
                    return@Column
                }
                when (route) {
                    Route.Folders -> folders()
                    Route.List -> list()
                    // Jumping to a folder from the manager lands on its list, with
                    // the sidebar beneath it for back.
                    Route.FolderManager ->
                        FolderManagerScreen(
                            state = state,
                            onOpenFolder = { stack = mailStack() },
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
                    is Route.Reader -> reader(route)
                    Route.Settings -> SettingsScreen(state = state, onClose = ::back)
                Route.Outbox -> OutboxScreen(state = state)
                Route.Contacts -> ContactsScreen()
                    is Route.Composer ->
                        ComposerScreen(
                            state = state,
                            seed = route.seed,
                            accountId = route.accountId,
                            folderId = route.folderId,
                            onClose = ::back,
                        )
                }
            }
        }
    }
}

// Hides the status and navigation bars while [on]; a swipe from the edge
// shows them for a moment, as in any full-screen viewer.
@Composable
private fun ImmersiveMode(on: Boolean) {
    val view = LocalView.current
    DisposableEffect(on) {
        val window = view.context.findActivity()?.window
        val bars = window?.let { WindowCompat.getInsetsController(it, view) }
        if (on && bars != null) {
            bars.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            bars.hide(WindowInsetsCompat.Type.systemBars())
        }
        onDispose { if (on) bars?.show(WindowInsetsCompat.Type.systemBars()) }
    }
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
