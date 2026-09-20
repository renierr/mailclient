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
import mailclient.ui.MailApp
import mailclient.ui.MailDarkColors
import mailclient.ui.MailLightColors

/**
 * Desktop entry point (Compose Multiplatform, JVM). Same SQLite file as
 * the QML app via MAILCLIENT_DB; backend binary via MAILFEED_BIN.
 */
fun main() = application {
    val repo = remember { CliMailRepository(MailfeedLocator.find()) }
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
