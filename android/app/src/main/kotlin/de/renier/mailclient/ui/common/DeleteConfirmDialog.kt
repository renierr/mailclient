package de.renier.mailclient.ui.common

import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonColors
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable

/** A filled button in the error colours, for an action that destroys data. */
@Composable
fun destructiveButtonColors(): ButtonColors = ButtonDefaults.buttonColors(
    containerColor = MaterialTheme.colorScheme.error,
    contentColor = MaterialTheme.colorScheme.onError,
)

// Destructive-action confirm (trash per the delete preference, purge, draft
// delete, forgetting an outbox entry, removing contacts, maintenance): states
// exactly what goes where, like the desktop dialogs, with the confirm in the
// error colours.
@Composable
fun DeleteConfirmDialog(
    title: String,
    text: String,
    confirmLabel: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = {
            Button(onClick = onConfirm, colors = destructiveButtonColors()) { Text(confirmLabel) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

/**
 * Deleting mail: one message by [subject], or [count] selected ones when
 * [subject] is null. [permanent] destroys on the server instead of moving
 * to Trash (the core's `delete_prompt`); a purge is always permanent.
 */
@Composable
fun MessageDeleteConfirm(
    subject: String?,
    count: Int,
    permanent: Boolean,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    val what = if (subject != null) "“$subject”" else "$count messages"
    DeleteConfirmDialog(
        title = if (permanent) "Delete permanently?" else "Move to Trash?",
        text = when {
            permanent -> "$what will be destroyed on the server. This cannot be undone."
            subject != null -> "$what will be moved to Trash."
            else -> "$what will be moved to Trash. You can undo this."
        },
        confirmLabel = if (permanent) "Delete permanently" else "Move to Trash",
        onConfirm = onConfirm,
        onDismiss = onDismiss,
    )
}
