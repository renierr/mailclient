package mailclient.ui

import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.ui.graphics.Color

// Design tokens mirroring crates/mailapp/qml/Theme.qml (single source of
// truth stays QML until the Kotlin client owns its own theme pass).

val MailLightColors = lightColorScheme(
    primary = Color(0xFF2F6FED),
    onPrimary = Color.White,
    background = Color(0xFFF6F7F9),
    surface = Color.White,
    onSurface = Color(0xFF1B1E24),
    surfaceVariant = Color(0xFFE9ECF1),
    outline = Color(0xFFD8DDE5),
)

val MailDarkColors = darkColorScheme(
    primary = Color(0xFF6B9BFF),
    onPrimary = Color(0xFF0B0E13),
    background = Color(0xFF14171C),
    surface = Color(0xFF1C2027),
    onSurface = Color(0xFFE8EAF0),
    surfaceVariant = Color(0xFF262B34),
    outline = Color(0xFF343B47),
)

/** Accent bar / unread dot color, theme-independent. */
val UnreadAccent = Color(0xFF2F6FED)

/** Star on/off colors. */
val StarOn = Color(0xFFE8A33D)
val StarOff = Color(0xFF9AA1AD)

/** Human role label for a folder role key (matches FolderRole::as_str). */
fun roleLabel(role: String): String = when (role) {
    "inbox" -> "Inbox"
    "sent" -> "Sent"
    "drafts" -> "Drafts"
    "trash" -> "Trash"
    "junk" -> "Junk"
    "archive" -> "Archive"
    else -> role.ifBlank { "Folder" }
}

/** Sort folders like the QML sidebar: well-known roles first, then by name. */
private val ROLE_ORDER = listOf("inbox", "drafts", "sent", "archive", "junk", "trash")

fun sortedFolders(folders: List<mailclient.models.Folder>): List<mailclient.models.Folder> {
    return folders.sortedWith(
        compareBy(
            { ROLE_ORDER.indexOf(it.role).let { i -> if (i < 0) 99 else i } },
            { it.name.lowercase() },
        ),
    )
}
