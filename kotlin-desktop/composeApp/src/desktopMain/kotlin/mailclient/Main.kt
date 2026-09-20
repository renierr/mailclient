package mailclient

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.remember
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.application
import androidx.compose.ui.window.rememberWindowState
import mailclient.repo.CliMailRepository
import mailclient.repo.MailfeedLocator
import mailclient.repo.NativeMailRepository
import mailclient.ui.MailApp
import mailclient.ui.MailDarkColors
import mailclient.ui.MailLightColors

/**
 * Desktop entry point (Compose Multiplatform, JVM).
 * Uses in-process JNI (NativeMailRepository) for sub-millisecond execution,
 * with graceful fallback to CliMailRepository if needed.
 */
fun main() = application {
    val repo = remember {
        try {
            NativeMailRepository()
        } catch (e: Throwable) {
            System.err.println("Native JNI unavailable (${e.message}), falling back to CLI repository.")
            CliMailRepository(MailfeedLocator.find())
        }
    }
    Window(
        onCloseRequest = ::exitApplication,
        title = "Mailclient (Kotlin)",
        state = rememberWindowState(width = 1280.dp, height = 820.dp),
    ) {
        MaterialTheme(colorScheme = if (isSystemInDarkTheme()) MailDarkColors else MailLightColors) {
            MailApp(repo)
        }
    }
}
