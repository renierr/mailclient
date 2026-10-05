package de.renier.mailclient.ui.common

import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

// Pull down to sync: the mail panes' Sync, as on every Android mail app. The
// content must scroll (a LazyColumn), or the gesture has nothing to pull.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PullToSync(
    syncing: Boolean,
    onSync: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable BoxScope.() -> Unit,
) {
    PullToRefreshBox(
        isRefreshing = syncing,
        onRefresh = onSync,
        modifier = modifier.fillMaxSize(),
        content = content,
    )
}
