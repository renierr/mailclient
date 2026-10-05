package de.renier.mailclient.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R

/** One entry of the shell's overflow menu. */
data class ShellMenuItem(
    val label: String,
    val icon: Int,
    val checked: Boolean? = null,
    val onClick: () -> Unit,
)

@Composable
fun ShellIcon(res: Int, description: String?, modifier: Modifier = Modifier) {
    Icon(painter = painterResource(res), contentDescription = description, modifier = modifier)
}

// The mail panes' bar: one rounded search field holding navigation, Sync and
// the tools menu — the phone form of the Qt toolbar (AGENTS.md: back, search,
// Sync, tools; Compose is the FAB). No folder name here: the list header
// shows it.
@Composable
fun SearchTopBar(
    query: String,
    placeholder: String,
    canGoBack: Boolean,
    onBack: () -> Unit,
    onQuery: (String) -> Unit,
    onClear: () -> Unit,
    syncing: Boolean,
    onSync: () -> Unit,
    menu: List<ShellMenuItem>,
) {
    val focus = LocalFocusManager.current
    Surface(
        shape = RoundedCornerShape(28.dp),
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 8.dp)
            .height(56.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(horizontal = 4.dp)) {
            if (canGoBack) {
                IconButton(onClick = onBack) { ShellIcon(R.drawable.ic_arrow_back, "Back") }
            } else {
                Box(modifier = Modifier.size(48.dp), contentAlignment = Alignment.Center) {
                    ShellIcon(R.drawable.ic_search, null, Modifier.size(22.dp))
                }
            }
            Box(modifier = Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
                if (query.isEmpty()) {
                    Text(
                        placeholder,
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                BasicTextField(
                    value = query,
                    onValueChange = onQuery,
                    singleLine = true,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(
                        color = MaterialTheme.colorScheme.onSurface,
                    ),
                    cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    keyboardActions = KeyboardActions(onSearch = { focus.clearFocus() }),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            if (query.isNotEmpty()) {
                IconButton(onClick = onClear) { ShellIcon(R.drawable.ic_close, "Clear search") }
            }
            IconButton(onClick = onSync, enabled = !syncing) {
                if (syncing) {
                    CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
                } else {
                    ShellIcon(R.drawable.ic_sync, "Sync")
                }
            }
            OverflowMenu(menu)
        }
    }
}

// Every other page (manager, accounts, setup, dev): back plus a title.
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PageTopBar(title: String, onBack: () -> Unit) {
    TopAppBar(
        title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        navigationIcon = {
            IconButton(onClick = onBack) { ShellIcon(R.drawable.ic_arrow_back, "Back") }
        },
    )
}

// The menu sits in a Box with its button, so it opens under the button
// instead of at the window's corner.
@Composable
private fun OverflowMenu(items: List<ShellMenuItem>) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) { ShellIcon(R.drawable.ic_more_vert, "More") }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            items.forEachIndexed { i, item ->
                // The scope toggle leads; a divider sets it off from the tools.
                if (i > 0 && items[i - 1].checked != null && item.checked == null) HorizontalDivider()
                DropdownMenuItem(
                    text = { Text(item.label) },
                    leadingIcon = { ShellIcon(item.icon, null) },
                    trailingIcon = if (item.checked == true) {
                        { ShellIcon(R.drawable.ic_check, "On") }
                    } else {
                        null
                    },
                    onClick = {
                        open = false
                        item.onClick()
                    },
                )
            }
        }
    }
}

// Bottom strip: only there when it has something to say — a running job's
// progress, the last error, or mail waiting in the outbox. A phone has no
// room for a permanent "Ready".
@Composable
fun StatusStrip(
    text: String,
    error: Boolean,
    busy: Boolean,
    outboxPending: Int,
    outboxFailed: Boolean,
    onOutbox: () -> Unit,
) {
    val showText = busy || error
    if (!showText && outboxPending == 0) return
    Surface(color = MaterialTheme.colorScheme.surfaceContainer) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 4.dp)
                .height(40.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                if (showText) text else "",
                color = if (error) MaterialTheme.colorScheme.error
                else MaterialTheme.colorScheme.onSurfaceVariant,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (outboxPending > 0) {
                AssistChip(
                    onClick = onOutbox,
                    label = { Text("Outbox $outboxPending") },
                    leadingIcon = { ShellIcon(R.drawable.ic_send, null, Modifier.size(16.dp)) },
                    colors = if (outboxFailed) {
                        AssistChipDefaults.assistChipColors(
                            labelColor = MaterialTheme.colorScheme.error,
                            leadingIconContentColor = MaterialTheme.colorScheme.error,
                        )
                    } else {
                        AssistChipDefaults.assistChipColors()
                    },
                )
            }
        }
    }
}
