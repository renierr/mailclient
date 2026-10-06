package de.renier.mailclient.ui.common

import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

// Pull down to sync: the mail panes' Sync, as on every Android mail app. The
// content must scroll (a LazyColumn), or the gesture has nothing to pull.
//
// The pull spinner only answers the gesture and snaps back on release: a
// running job shows as the shell's header line and status strip, which
// follow the core's in-flight table. A spinner tied to the job would sit
// over the list for a whole account sync, and stick whenever the pull was
// refused because the same job is already running.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PullToSync(
    onSync: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable BoxScope.() -> Unit,
) {
    PullToRefreshBox(
        isRefreshing = false,
        onRefresh = onSync,
        modifier = modifier.fillMaxSize(),
        content = content,
    )
}
