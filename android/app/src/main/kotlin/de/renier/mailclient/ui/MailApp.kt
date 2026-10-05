package de.renier.mailclient.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import de.renier.mailclient.ui.shell.MailShell
import de.renier.mailclient.ui.theme.MailTheme

// Root: theme around the shell. The 0x probe cards live one tools-menu tap
// away on the Dev route until their screens land, then go.
@Composable
fun MailApp(openPayload: String?, onConsumeOpen: () -> Unit) {
    MailTheme {
        Surface(modifier = Modifier.fillMaxSize()) {
            MailShell(openPayload = openPayload, onConsumeOpen = onConsumeOpen)
        }
    }
}
