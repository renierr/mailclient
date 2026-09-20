package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import mailclient.models.Account

/**
 * Composer (QML Composer.qml) and account manager (QML Accounts.qml).
 */
@Composable
fun ComposerDialog(
    from: String,
    initialTo: String = "",
    initialSubject: String = "",
    initialBody: String = "",
    onSend: (to: String, cc: String, bcc: String, subject: String, body: String) -> Unit,
    onClose: () -> Unit,
) {
    var to by remember { mutableStateOf(initialTo) }
    var cc by remember { mutableStateOf("") }
    var bcc by remember { mutableStateOf("") }
    var showCcBcc by remember { mutableStateOf(false) }
    var subject by remember { mutableStateOf(initialSubject) }
    var body by remember { mutableStateOf(initialBody) }
    var sending by remember { mutableStateOf(false) }

    Dialog(onDismissRequest = onClose) {
        Column(
            Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surface, RoundedCornerShape(12.dp))
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text("Compose", fontWeight = FontWeight.Bold, fontSize = 18.sp)
            Row(
                Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("From: $from", fontSize = 13.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                TextButton(onClick = { showCcBcc = !showCcBcc }) {
                    Text(if (showCcBcc) "Hide Cc/Bcc" else "Cc/Bcc")
                }
            }
            OutlinedTextField(
                value = to,
                onValueChange = { to = it },
                label = { Text("To") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            if (showCcBcc) {
                OutlinedTextField(
                    value = cc,
                    onValueChange = { cc = it },
                    label = { Text("Cc") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = bcc,
                    onValueChange = { bcc = it },
                    label = { Text("Bcc") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            OutlinedTextField(
                value = subject,
                onValueChange = { subject = it },
                label = { Text("Subject") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = body,
                onValueChange = { body = it },
                label = { Text("Body") },
                minLines = 8,
                modifier = Modifier.fillMaxWidth(),
            )
            Row(
                horizontalArrangement = Arrangement.End,
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth(),
            ) {
                TextButton(onClick = onClose, enabled = !sending) { Text("Cancel") }
                Spacer(Modifier.width(8.dp))
                Button(
                    onClick = {
                        if (to.isNotBlank() || cc.isNotBlank() || bcc.isNotBlank()) {
                            sending = true
                            onSend(to, cc, bcc, subject, body)
                        }
                    },
                    enabled = !sending && (to.isNotBlank() || cc.isNotBlank() || bcc.isNotBlank()),
                ) {
                    if (sending) {
                        CircularProgressIndicator(
                            Modifier.size(16.dp),
                            strokeWidth = 2.dp,
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                        Spacer(Modifier.width(6.dp))
                    }
                    Text("Send")
                }
            }
        }
    }
}

@Composable
fun AccountsDialog(
    accounts: List<Account>,
    activeId: Long?,
    onSelect: (Long) -> Unit,
    onClose: () -> Unit,
) {
    Dialog(onDismissRequest = onClose) {
        Column(
            Modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surface, RoundedCornerShape(12.dp))
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text("Accounts", fontWeight = FontWeight.Bold, fontSize = 18.sp)
            LazyColumn {
                items(accounts, key = { it.id }) { a ->
                    Row(
                        Modifier.fillMaxWidth().padding(vertical = 8.dp),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text(a.name, fontWeight = FontWeight.Bold, fontSize = 14.sp)
                            Text(
                                a.email,
                                fontSize = 12.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        if (a.id == activeId) {
                            Text("active", fontSize = 12.sp, color = UnreadAccent, fontWeight = FontWeight.Bold)
                        } else {
                            TextButton(onClick = { onSelect(a.id) }) { Text("Switch") }
                        }
                    }
                }
            }
            Text(
                "Accounts are managed in ~/.local/share/mailclient or dev DB in data/.",
                fontSize = 11.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Row(horizontalArrangement = Arrangement.End, modifier = Modifier.fillMaxWidth()) {
                TextButton(onClick = onClose) { Text("Close") }
            }
        }
    }
}

