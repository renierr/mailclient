package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import mailclient.models.Folder

// Design tokens mirroring crates/mailapp/qml/Theme.qml
// Single source of truth for color, spacing, radius and typography.

object MailTheme {
    // Dark surfaces
    val DarkBg = Color(0xFF16181D)
    val DarkBgAlt = Color(0xFF1B1E24)
    val DarkBgRaised = Color(0xFF22262D)
    val DarkBorder = Color(0xFF2C323B)
    val DarkHover = Color(0xFF232830)
    val DarkSelected = Color(0xFF25314A)

    // Dark content
    val DarkText = Color(0xFFE6E8EB)
    val DarkTextMuted = Color(0xFF98A1AD)
    val DarkAccent = Color(0xFF5C93FF)
    val DarkDanger = Color(0xFFF2777A)

    // Light surfaces
    val LightBg = Color(0xFFFFFFFF)
    val LightBgAlt = Color(0xFFF7F8FA)
    val LightBgRaised = Color(0xFFFFFFFF)
    val LightBorder = Color(0xFFE3E6EA)
    val LightHover = Color(0xFFF1F3F6)
    val LightSelected = Color(0xFFE7EFFF)

    // Light content
    val LightText = Color(0xFF1C1F23)
    val LightTextMuted = Color(0xFF6B7280)
    val LightAccent = Color(0xFF2F6FED)
    val LightDanger = Color(0xFFC8342F)

    // Shared tokens
    val Star = Color(0xFFF0A92C)
    val StarMuted = Color(0xFF8C95A3)
    val UnreadAccent = Color(0xFF2F6FED)
}

val MailLightColors = lightColorScheme(
    primary = MailTheme.LightAccent,
    onPrimary = Color.White,
    background = MailTheme.LightBg,
    surface = MailTheme.LightBgRaised,
    onSurface = MailTheme.LightText,
    surfaceVariant = MailTheme.LightBgAlt,
    onSurfaceVariant = MailTheme.LightTextMuted,
    outline = MailTheme.LightBorder,
    error = MailTheme.LightDanger,
)

val MailDarkColors = darkColorScheme(
    primary = MailTheme.DarkAccent,
    onPrimary = Color.White,
    background = MailTheme.DarkBg,
    surface = MailTheme.DarkBgRaised,
    onSurface = MailTheme.DarkText,
    surfaceVariant = MailTheme.DarkBgAlt,
    onSurfaceVariant = MailTheme.DarkTextMuted,
    outline = MailTheme.DarkBorder,
    error = MailTheme.DarkDanger,
)

/** Star on/off colors. */
val StarOn = MailTheme.Star
val StarOff = MailTheme.StarMuted
val UnreadAccent = MailTheme.UnreadAccent

/** Folder icon matching QML Sidebar.qml. */
fun folderIcon(role: String): String = when (role.lowercase()) {
    "inbox" -> "📥"
    "drafts" -> "📝"
    "sent" -> "📤"
    "archive" -> "🗄"
    "junk" -> "🚫"
    "trash" -> "🗑"
    else -> "📁"
}

/** Human role label for a folder role key. */
fun roleLabel(role: String): String = when (role.lowercase()) {
    "inbox" -> "Inbox"
    "sent" -> "Sent"
    "drafts" -> "Drafts"
    "trash" -> "Trash"
    "junk" -> "Junk"
    "archive" -> "Archive"
    else -> role.ifBlank { "Folder" }
}

/** Deterministic avatar color per sender (same name, same colour). */
fun avatarColor(seed: String, isDark: Boolean = true): Color {
    val s = seed.ifBlank { "?" }
    var h = 0
    for (ch in s) {
        h = (h * 31 + ch.code) % 360
    }
    if (h < 0) h += 360
    val sat = if (isDark) 0.42f else 0.50f
    val light = if (isDark) 0.46f else 0.52f
    return hslToRgb(h.toFloat(), sat, light)
}

private fun hslToRgb(h: Float, s: Float, l: Float): Color {
    val c = (1f - kotlin.math.abs(2f * l - 1f)) * s
    val x = c * (1f - kotlin.math.abs((h / 60f) % 2f - 1f))
    val m = l - c / 2f
    val (r1, g1, b1) = when {
        h < 60f -> Triple(c, x, 0f)
        h < 120f -> Triple(x, c, 0f)
        h < 180f -> Triple(0f, c, x)
        h < 240f -> Triple(0f, x, c)
        h < 300f -> Triple(x, 0f, c)
        else -> Triple(c, 0f, x)
    }
    return Color(
        red = (r1 + m).coerceIn(0f, 1f),
        green = (g1 + m).coerceIn(0f, 1f),
        blue = (b1 + m).coerceIn(0f, 1f),
    )
}

/** Clean circular avatar displaying sender initial. */
@Composable
fun SenderAvatar(
    seed: String,
    modifier: Modifier = Modifier,
    size: Dp = 32.dp,
    fontSize: TextUnit = 13.sp,
) {
    val isDark = isSystemInDarkTheme()
    val bg = avatarColor(seed, isDark)
    val initial = seed.trimStart { !it.isLetterOrDigit() }.take(1).uppercase().ifEmpty { "?" }

    Box(
        modifier = modifier
            .size(size)
            .clip(CircleShape)
            .background(bg),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = initial,
            color = Color.White,
            fontWeight = FontWeight.Bold,
            fontSize = fontSize,
        )
    }
}

/** Sort folders like the QML sidebar: well-known roles first, then by name. */
private val ROLE_ORDER = listOf("inbox", "drafts", "sent", "archive", "junk", "trash")

fun sortedFolders(folders: List<Folder>): List<Folder> {
    return folders.sortedWith(
        compareBy(
            { ROLE_ORDER.indexOf(it.role.lowercase()).let { i -> if (i < 0) 99 else i } },
            { it.name.lowercase() },
        ),
    )
}
