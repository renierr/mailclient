package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import mailclient.models.Folder

/** Left pane: folder tree with unread pills (QML Sidebar.qml). */
@Composable
fun Sidebar(
    folders: List<Folder>,
    activeId: Long?,
    onSelect: (Long) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier.background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.35f))) {
        Text(
            "Folders",
            fontWeight = FontWeight.Bold,
            fontSize = 13.sp,
            modifier = Modifier.padding(12.dp, 10.dp, 12.dp, 4.dp),
        )
        LazyColumn {
            items(folders, key = { it.id }) { f ->
                val active = f.id == activeId
                Row(
                    Modifier.fillMaxWidth()
                        .clickable { onSelect(f.id) }
                        .background(
                            if (active) MaterialTheme.colorScheme.primary.copy(alpha = 0.14f)
                            else androidx.compose.ui.graphics.Color.Transparent,
                        )
                        .padding(start = (12 + f.depth() * 14).dp, end = 12.dp, top = 7.dp, bottom = 7.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    // Accent bar for the active folder (QML unread-dot idiom).
                    Box(
                        Modifier
                            .clip(CircleShape)
                            .background(
                                if (active) UnreadAccent
                                else androidx.compose.ui.graphics.Color.Transparent,
                            )
                            .padding(2.dp),
                    )
                    Column(Modifier.weight(1f)) {
                        Text(
                            f.displayName(),
                            fontWeight = if (f.unread > 0) FontWeight.Bold else FontWeight.Normal,
                            fontSize = 14.sp,
                            maxLines = 1,
                        )
                        if (f.role != "custom") {
                            Text(
                                roleLabel(f.role),
                                fontSize = 11.sp,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                    }
                    if (f.unread > 0) UnreadPill(f.unread.toString())
                }
            }
            if (folders.isEmpty()) {
                item {
                    Text(
                        "No folders — sync first.",
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(12.dp),
                    )
                }
            }
        }
        Spacer(Modifier.weight(1f))
    }
}
