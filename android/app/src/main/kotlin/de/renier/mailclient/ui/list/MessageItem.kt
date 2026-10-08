package de.renier.mailclient.ui.list

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.Avatar
import de.renier.mailclient.ui.state.MessageRow
import de.renier.mailclient.ui.theme.starColor

@Composable
internal fun MessageItem(
    m: MessageRow,
    folderLabel: String?,
    selected: Boolean?,
    modifier: Modifier,
    onToggle: () -> Unit,
    // List density "compact": no snippet line, tighter rows.
    compact: Boolean = false,
    // The ⋮ menu; hidden while selecting (the bulk bar acts then).
    permanent: Boolean = false,
    onAction: ((RowAction) -> Unit)? = null,
) {
    val scheme = MaterialTheme.colorScheme
    // Qt's and Flutter's row: a small avatar at the top left with the
    // unread dot on its corner and the paperclip under it; sender (and
    // star) with the date on the right, subject with the ⋮ under the date,
    // then the snippet.
    Row(
        modifier = modifier
            .fillMaxWidth()
            .padding(start = 12.dp, end = 4.dp, top = if (compact) 4.dp else 8.dp, bottom = if (compact) 4.dp else 8.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.width(32.dp),
        ) {
            if (selected != null) {
                Checkbox(checked = selected, onCheckedChange = { onToggle() }, modifier = Modifier.size(32.dp))
            } else {
                Box {
                    Avatar(initials = m.initials, avatarLight = m.avatarLight, avatarDark = m.avatarDark, size = 28.dp)
                    if (m.unread) {
                        Canvas(
                            modifier = Modifier
                                .size(10.dp)
                                .align(Alignment.TopStart)
                                .offset(x = (-2).dp, y = (-2).dp),
                        ) {
                            drawCircle(color = scheme.surface, radius = size.minDimension / 2)
                            drawCircle(color = scheme.primary, radius = size.minDimension / 2 - 1.5.dp.toPx())
                        }
                    }
                }
            }
            if (m.hasAttachments) {
                Icon(
                    painter = painterResource(R.drawable.ic_attach),
                    contentDescription = "Has attachments",
                    tint = scheme.outline,
                    modifier = Modifier.padding(top = 6.dp).size(14.dp),
                )
            }
        }
        Column(modifier = Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Row(modifier = Modifier.weight(1f), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        m.fromName.ifEmpty { m.from },
                        style = MaterialTheme.typography.bodyLarge,
                        fontWeight = if (m.unread) FontWeight.Bold else null,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false),
                    )
                    if (m.starred) {
                        Icon(
                            painter = painterResource(R.drawable.ic_star),
                            contentDescription = "Starred",
                            tint = starColor(true),
                            modifier = Modifier.padding(start = 4.dp).size(14.dp),
                        )
                    }
                }
                Text(
                    m.date,
                    style = MaterialTheme.typography.labelMedium,
                    fontWeight = if (m.unread) FontWeight.Bold else null,
                    color = if (m.unread) scheme.primary else scheme.outline,
                    modifier = Modifier.padding(start = 8.dp, end = 8.dp),
                )
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    m.subject,
                    style = MaterialTheme.typography.bodyMedium,
                    fontWeight = if (m.unread) FontWeight.SemiBold else null,
                    color = scheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (selected == null && onAction != null) {
                    RowMenuButton(m, permanent, onAction)
                }
            }
            if (!compact && m.snippet.isNotEmpty()) {
                Text(
                    m.snippet,
                    style = MaterialTheme.typography.bodyMedium,
                    color = scheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(end = 8.dp),
                )
            }
            folderLabel?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.labelSmall,
                    color = scheme.primary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}
