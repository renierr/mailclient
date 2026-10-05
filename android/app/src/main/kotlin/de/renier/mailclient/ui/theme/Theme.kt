package de.renier.mailclient.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

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
    MaterialTheme(
        colorScheme = if (dark) DarkScheme else LightScheme,
        content = content,
    )
}

// Amber star state, shared with the reader's starred marker.
@Composable
fun starColor(starred: Boolean): Color =
    if (starred) StarAmber else MaterialTheme.colorScheme.onSurfaceVariant
