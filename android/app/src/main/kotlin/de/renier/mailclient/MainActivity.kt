package de.renier.mailclient

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import de.renier.mailclient.ui.MailApp

// The one activity of the native Android frontend: every screen, the reader
// included, is a pane of the Compose shell. Notification taps land here
// (MailNotifier ACTION_OPEN) and the shell opens the message they name.
class MainActivity : ComponentActivity() {
    // A notification tap, consumed by the shell once its lists are up.
    private var openPayload by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        handleIntent(intent)
        setContent {
            MailApp(
                openPayload = openPayload,
                onConsumeOpen = { openPayload = null },
            )
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        if (intent?.action == MailNotifier.ACTION_OPEN) {
            openPayload = intent.getStringExtra(MailNotifier.EXTRA_PAYLOAD)
        }
    }
}
