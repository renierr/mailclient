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

// Compose launcher for the native Android frontend (branch
// `feature/native-android`). Home today, the full shell screen by screen;
// the reader already lives in ReaderActivity. This keeps the contract the
// moved backend files expect: notification taps land here (MailNotifier
// ACTION_OPEN) and ReaderActivity reports its shell-flow delegations
// (ACTION_READER) and mutations (readerDirty) through this activity.
class MainActivity : ComponentActivity() {
    // A shell flow the native reader cannot do itself (composer,
    // find-similar): shown as a placeholder until that screen exists.
    private var delegatePayload by mutableStateOf<String?>(null)

    // A notification tap that arrived while the lists were not up yet.
    // Carries the open target for the future message-list screen.
    private var openPayload by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        handleIntent(intent)
        setContent {
            MailApp(
                delegatePayload = delegatePayload,
                onConsumeDelegate = { delegatePayload = null },
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
        when (intent?.action) {
            ACTION_READER ->
                delegatePayload = intent.getStringExtra(EXTRA_READER_PAYLOAD)
            MailNotifier.ACTION_OPEN ->
                openPayload = intent.getStringExtra(MailNotifier.EXTRA_PAYLOAD)
        }
    }

    companion object {
        // A shell flow the reader delegates back (composer, find-similar),
        // as a JSON payload: {"kind","accountId","folderId","uid"}.
        const val ACTION_READER = "de.renier.mailclient.READER_ACTION"
        const val EXTRA_READER_PAYLOAD = "reader_payload"

        // Set by ReaderActivity when it changed mail; Home reloads its
        // lists on resume when this is set. Same process, no IPC.
        @Volatile var readerDirty = false
    }
}
