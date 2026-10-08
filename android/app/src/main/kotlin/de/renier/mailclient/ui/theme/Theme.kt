package de.renier.mailclient.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.platform.LocalContext

// Brand blue shared with the launcher icon sources (#3B82F6).
private val LightPrimary = Color(0xFF3B82F6)
private val DarkPrimary = Color(0xFF7AB4FF)
private val LightError = Color(0xFFB3261E)
private val DarkError = Color(0xFFF2B8B5)
private val StarAmber = Color(0xFFFFA000)

private val LightScheme = lightColorScheme(
    primary = LightPrimary,
    error = LightError,
)

private val DarkScheme = darkColorScheme(
    primary = DarkPrimary,
    error = DarkError,
)

@Composable
fun MailTheme(
    dark: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    // Material You on Android 12+ (wallpaper colours, like the system
    // apps); the brand scheme below that.
    val context = LocalContext.current
    val scheme = when {
        android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S ->
            if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        dark -> DarkScheme
        else -> LightScheme
    }
    MaterialTheme(
        colorScheme = scheme,
        content = content,
    )
}

// Amber star state, shared with the reader's starred marker.
@Composable
fun starColor(starred: Boolean): Color =
    if (starred) StarAmber else MaterialTheme.colorScheme.onSurfaceVariant

private val LightSuccess = Color(0xFF1E7A3C)
private val DarkSuccess = Color(0xFF6CC88F)
private val LightWarning = Color(0xFF9A5B00)
private val DarkWarning = Color(0xFFF0B35A)

// Colour for a core-given tone or outcome on the reader cards:
// `positive`/`delivered` green, `negative`/`failed` error, `delayed` amber,
// anything else the primary colour.
@Composable
fun toneColor(tone: String): Color {
    val scheme = MaterialTheme.colorScheme
    val dark = scheme.background.luminance() < 0.5f
    return when (tone) {
        "positive", "delivered" -> if (dark) DarkSuccess else LightSuccess
        "negative", "failed" -> scheme.error
        "delayed" -> if (dark) DarkWarning else LightWarning
        else -> scheme.primary
    }
}
