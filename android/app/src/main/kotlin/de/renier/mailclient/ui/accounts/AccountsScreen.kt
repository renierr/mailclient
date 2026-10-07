package de.renier.mailclient.ui.accounts

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.ui.common.Avatar
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.removeAccount
import de.renier.mailclient.ui.state.selectAccount

// Step 2 accounts manager: switch, edit, remove (confirmed), add. The setup
// form itself is AccountSetupScreen; secrets never come back out.
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AccountsScreen(
    state: MailState,
    onAdd: () -> Unit,
    onEdit: (Long) -> Unit,
) {
    var pendingRemove by remember { mutableStateOf<Long?>(null) }

    Column(modifier = Modifier.fillMaxSize()) {
        LazyColumn(modifier = Modifier.weight(1f)) {
            items(state.accounts, key = { it.id }) { a ->
                val active = a.id == state.activeAccountId
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .clickable(enabled = !active) { state.selectAccount(a.id) }
                        .padding(horizontal = 16.dp, vertical = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Avatar(
                        initials = a.initials,
                        avatarLight = a.avatarLight,
                        avatarDark = a.avatarDark,
                        size = 44.dp,
                    )
                    // Actions sit under the text, not beside it: three
                    // buttons in the row leave a 360dp phone no room for
                    // the name.
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            a.name,
                            style = MaterialTheme.typography.titleMedium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            a.email + if (active) " · active" else "",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        FlowRow {
                            if (!active) {
                                TextButton(onClick = { state.selectAccount(a.id) }) { Text("Use") }
                            }
                            TextButton(onClick = { onEdit(a.id) }) { Text("Edit") }
                            TextButton(onClick = { pendingRemove = a.id }) { Text("Remove") }
                        }
                    }
                }
                HorizontalDivider(modifier = Modifier.padding(start = 72.dp))
            }
        }
        TextButton(
            onClick = onAdd,
            modifier = Modifier.fillMaxWidth().padding(8.dp),
        ) {
            Text("Add account…")
        }
    }

    val removeId = pendingRemove
    if (removeId != null) {
        val target = state.accounts.firstOrNull { it.id == removeId }
        AlertDialog(
            onDismissRequest = { pendingRemove = null },
            title = { Text("Remove account?") },
            text = {
                Text(
                    "Drops ${target?.email ?: "this account"} with its local cache " +
                        "and stored passwords. The server is untouched.",
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    pendingRemove = null
                    state.removeAccount(removeId)
                }) { Text("Remove", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { pendingRemove = null }) { Text("Cancel") }
            },
        )
    }
}
