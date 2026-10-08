package de.renier.mailclient.ui.list

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import kotlinx.coroutines.launch

// What sits around the rows: the load-older footer, the empty state and
// the floating jump buttons.

// Lists at most this long get no jump buttons.
private const val JUMP_MIN_ITEMS = 12

// Always-on footer (Qt loadOlderBar): the server status stays visible even
// with nothing left to load, so the list never ends in silence. [label] is
// the core's (`feed::older_label`).
@Composable
internal fun LoadOlderFooter(
    label: String,
    unchecked: Boolean,
    canLoad: Boolean,
    busy: Boolean,
    onLoad: () -> Unit,
) {
    Column(
        modifier = Modifier.fillMaxWidth().padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(
            label,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        if (canLoad) {
            // Qt's wording: an unchecked folder asks the server first; any
            // running job waits.
            OutlinedButton(onClick = onLoad, enabled = !busy, modifier = Modifier.padding(top = 8.dp)) {
                Text(
                    when {
                        busy -> "Loading…"
                        unchecked -> "Check server"
                        else -> "Load older"
                    },
                )
            }
        }
    }
}

// Qt's empty-list block: the core's words, plus how to fetch mail when the
// folder itself is empty (pull here, ⟳ on the desktop).
@Composable
internal fun EmptyList(text: String, syncHint: Boolean, modifier: Modifier) {
    Column(
        modifier = modifier.padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Icon(
            painterResource(if (syncHint) R.drawable.ic_inbox else R.drawable.ic_search),
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.5f),
            modifier = Modifier.size(32.dp),
        )
        Text(
            text,
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        if (syncHint) {
            Text(
                "Pull down to sync",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

// Jump to top / bottom (Qt ScrollJumpButtons): only over long lists, each
// end only while it is off-screen. The scroll position is read through
// derived state here, so scrolling recomposes these buttons, not the list.
@Composable
internal fun ScrollJumpButtons(listState: LazyListState, modifier: Modifier = Modifier) {
    val scope = rememberCoroutineScope()
    val up by remember(listState) {
        derivedStateOf { listState.layoutInfo.totalItemsCount > JUMP_MIN_ITEMS && listState.canScrollBackward }
    }
    val down by remember(listState) {
        derivedStateOf { listState.layoutInfo.totalItemsCount > JUMP_MIN_ITEMS && listState.canScrollForward }
    }
    if (!up && !down) return
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        if (up) {
            FilledTonalIconButton(onClick = { scope.launch { listState.scrollToItem(0) } }) {
                Icon(painterResource(R.drawable.ic_arrow_up), contentDescription = "Jump to top")
            }
        }
        if (down) {
            FilledTonalIconButton(
                onClick = { scope.launch { listState.scrollToItem(listState.layoutInfo.totalItemsCount - 1) } },
            ) {
                Icon(painterResource(R.drawable.ic_arrow_down), contentDescription = "Jump to bottom")
            }
        }
    }
}
