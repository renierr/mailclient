package de.renier.mailclient.ui.common

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.dp

/**
 * Leaving a page with unsaved work. With [onSave] the dialog offers
 * [saveLabel] first, then Cancel and Discard; without, only Discard and
 * Keep editing. Every choice closes the dialog through its own callback.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun UnsavedChangesDialog(
    title: String,
    text: String,
    onDiscard: () -> Unit,
    onDismiss: () -> Unit,
    saveLabel: String = "Save",
    onSave: (() -> Unit)? = null,
) {
    val discard: @Composable () -> Unit = {
        TextButton(onClick = onDiscard) { Text("Discard", color = MaterialTheme.colorScheme.error) }
    }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = {
            if (onSave != null) Button(onClick = onSave) { Text(saveLabel) } else discard()
        },
        dismissButton = {
            // Wraps on narrow screens instead of pushing a button off.
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = onDismiss) { Text(if (onSave != null) "Cancel" else "Keep editing") }
                if (onSave != null) discard()
            }
        },
    )
}
