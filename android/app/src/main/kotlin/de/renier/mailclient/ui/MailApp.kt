package de.renier.mailclient.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.semantics.clearAndSetSemantics
import de.renier.mailclient.AppLock
import de.renier.mailclient.ui.lock.LockCover
import de.renier.mailclient.ui.shell.MailShell
import de.renier.mailclient.ui.theme.MailTheme

// Root: theme around the shell, and the app-lock cover over it. While the
// cover shows, the shell stays composed (drafts survive) but is hidden from
// accessibility services and gets no hardware-key input.
@Composable
fun MailApp(openPayload: String?, onConsumeOpen: () -> Unit) {
    MailTheme {
        val covered = AppLock.locked != false
        Box(modifier = Modifier.fillMaxSize()) {
            Surface(
                modifier = Modifier
                    .fillMaxSize()
                    .then(
                        if (covered) {
                            Modifier
                                .clearAndSetSemantics {}
                                .onPreviewKeyEvent { it.key != Key.Back }
                        } else {
                            Modifier
                        },
                    ),
            ) {
                MailShell(openPayload = openPayload, onConsumeOpen = onConsumeOpen)
            }
            LockCover()
        }
    }
}
