package de.renier.mailclient

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import de.renier.mailclient.ui.MailApp

// The one activity of the native Android frontend: every screen, the reader
// included, is a pane of the Compose shell. Notification taps and launcher
// shortcuts land here (MailNotifier ACTION_OPEN) and the shell opens what
// their payload names.
class MainActivity : ComponentActivity() {
    // A notification tap or launcher shortcut, consumed by the shell once
    // its lists are up.
    private var openPayload by mutableStateOf<String?>(null)

    // The screen-lock page AppLock falls back to (Android 9, no fingerprint).
    private lateinit var screenLock: ActivityResultLauncher<Intent>

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        screenLock = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
            AppLock.onScreenLockResult(it.resultCode == RESULT_OK)
        }
        AppLock.attach(this)
        handleIntent(intent)
        setContent {
            MailApp(
                openPayload = openPayload,
                onConsumeOpen = { openPayload = null },
            )
        }
    }

    fun confirmScreenLock(intent: Intent) = screenLock.launch(intent)

    override fun onStart() {
        super.onStart()
        AppLock.onStart(this)
    }

    // On screen: new mail found by a background check shows in the list
    // instead of alerting, and the notifications have done their job.
    override fun onResume() {
        super.onResume()
        MailNotifier.foreground = true
        MailNotifier.clear(this)
        AppLock.onResume(this)
    }

    override fun onPause() {
        MailNotifier.foreground = false
        super.onPause()
    }

    override fun onStop() {
        AppLock.onStop(this)
        super.onStop()
    }

    override fun onDestroy() {
        AppLock.detach(this)
        super.onDestroy()
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
