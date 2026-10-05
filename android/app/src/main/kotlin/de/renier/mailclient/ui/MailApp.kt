package de.renier.mailclient.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import de.renier.mailclient.ui.delegate.DelegateScreen
import de.renier.mailclient.ui.home.HomeScreen
import de.renier.mailclient.ui.theme.MailTheme

// Root: Home, or the placeholder for a shell flow the native reader
// delegated back (composer, find-similar) until that screen exists.
@Composable
fun MailApp(
    delegatePayload: String?,
    onConsumeDelegate: () -> Unit,
    openPayload: String?,
    onConsumeOpen: () -> Unit,
) {
    MailTheme {
        Surface(modifier = Modifier.fillMaxSize()) {
            if (delegatePayload != null) {
                DelegateScreen(payload = delegatePayload, onClose = onConsumeDelegate)
            } else {
                HomeScreen(openPayload = openPayload, onConsumeOpen = onConsumeOpen)
            }
        }
    }
}
