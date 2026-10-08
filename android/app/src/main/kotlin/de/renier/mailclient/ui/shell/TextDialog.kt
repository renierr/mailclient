package de.renier.mailclient.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.copyToClipboard

/**
 * A long text in full, selectable and scrolling: the status strip's details
 * (Copy offered) and the search syntax. [mono] for text aligned with spaces
 * or error output.
 */
@Composable
fun TextDialog(
    title: String,
    text: String,
    icon: Int,
    onDismiss: () -> Unit,
    error: Boolean = false,
    mono: Boolean = false,
    // The copy's toast, or null for no Copy button.
    onCopied: ((String) -> Unit)? = null,
) {
    val context = LocalContext.current
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = {
            Icon(
                painterResource(icon),
                null,
                tint = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        },
        title = { Text(title) },
        text = {
            SelectionContainer(modifier = Modifier.heightIn(max = 320.dp).verticalScroll(rememberScrollState())) {
                Text(
                    text,
                    style = MaterialTheme.typography.bodyMedium,
                    fontFamily = if (mono) FontFamily.Monospace else null,
                )
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Close") } },
        dismissButton = onCopied?.let { copied ->
            {
                TextButton(onClick = {
                    copyToClipboard(context, title, text)
                    copied("Copied to clipboard")
                }) {
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(painterResource(R.drawable.ic_content_copy), null, Modifier.size(18.dp))
                        Text("Copy", modifier = Modifier.padding(end = 2.dp))
                    }
                }
            }
        },
    )
}
