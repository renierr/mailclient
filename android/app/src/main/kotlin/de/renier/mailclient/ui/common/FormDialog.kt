package de.renier.mailclient.ui.common

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import de.renier.mailclient.R

/**
 * A small form (a field or two): a dialog where it fits, a full page with
 * the action in its bar on short screens (landscape phone), so the keyboard
 * never squeezes it (AGENTS.md: text input goes fullscreen there).
 * [onConfirm] decides itself whether to close (it may show an error).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FormDialog(
    title: String,
    confirmLabel: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    if (!isShortScreen()) {
        AlertDialog(
            onDismissRequest = onDismiss,
            title = { Text(title) },
            text = { Column(content = content) },
            confirmButton = { Button(onClick = onConfirm) { Text(confirmLabel) } },
            dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
        )
        return
    }
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        Surface(modifier = Modifier.fillMaxSize()) {
            Column(modifier = Modifier.safeDrawingPadding()) {
                TopAppBar(
                    title = { Text(title) },
                    navigationIcon = {
                        IconButton(onClick = onDismiss) { Icon(painterResource(R.drawable.ic_close), "Cancel") }
                    },
                    actions = { TextButton(onClick = onConfirm) { Text(confirmLabel) } },
                )
                Column(
                    modifier = Modifier
                        .verticalScroll(rememberScrollState())
                        .padding(horizontal = 16.dp, vertical = 8.dp),
                    content = content,
                )
            }
        }
    }
}
