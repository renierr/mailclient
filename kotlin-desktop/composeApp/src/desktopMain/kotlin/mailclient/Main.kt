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

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Density

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
        val density = LocalDensity.current
        println("Compose LocalDensity: density=${density.density}, fontScale=${density.fontScale}")
        val detectedScale = remember { detectSystemScale() }
        val effectiveScale = if (density.density <= 1.05f) detectedScale else 1.0f
        println("Applying effectiveScale: $effectiveScale")
        CompositionLocalProvider(
            LocalDensity provides Density(
                density = density.density * effectiveScale,
                fontScale = density.fontScale * effectiveScale,
            ),
        ) {
            MaterialTheme(colorScheme = if (isSystemInDarkTheme()) MailDarkColors else MailLightColors) {
                MailApp(repo)
            }
        }
    }
}

private fun detectSystemScale(): Float {
    System.getenv("MAILCLIENT_UI_SCALE")?.toFloatOrNull()?.let { return it }
    try {
        val proc = ProcessBuilder("hyprctl", "-j", "monitors").start()
        val text = proc.inputStream.bufferedReader().readText()
        val match = Regex("\"scale\":\\s*([0-9.]+)").find(text)
        if (match != null) {
            val scale = match.groupValues[1].toFloatOrNull()
            if (scale != null && scale > 0f) return scale
        }
    } catch (_: Exception) {}
    return 1.25f
}
