package mailclient.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import mailclient.models.Account
import mailclient.models.Folder

/**
 * Left pane: Account Chip + folder tree with icons and unread pills (QML Sidebar.qml).
 */
@Composable
fun Sidebar(
    accounts: List<Account>,
    activeAccountId: Long?,
    onSelectAccount: (Long) -> Unit,
    onManageAccounts: () -> Unit,
    folders: List<Folder>,
    activeFolderId: Long?,
    onSelectFolder: (Long) -> Unit,
    modifier: Modifier = Modifier,
) {
    var accountMenuExpanded by remember { mutableStateOf(false) }
    val currentAccount = accounts.firstOrNull { it.id == activeAccountId }
    val currentEmail = currentAccount?.email ?: ""

    Column(
        modifier
            .background(MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.45f))
            .padding(vertical = 8.dp),
    ) {
        // --- Account Chip (QML AccountChip) ---
        Box(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 10.dp, vertical = 4.dp),
        ) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(8.dp))
                    .border(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.5f), RoundedCornerShape(8.dp))
                    .clickable { accountMenuExpanded = true }
                    .padding(8.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                SenderAvatar(
                    seed = currentEmail,
                    size = 30.dp,
                    fontSize = 12.sp,
                )
                Column(Modifier.weight(1f)) {
                    Text(
                        text = if (currentEmail.isBlank()) "No account" else currentEmail,
                        fontWeight = FontWeight.Bold,
                        fontSize = 13.sp,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        text = if (accounts.size > 1) "${accounts.size} accounts — switch" else "Manage account",
                        fontSize = 11.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                Text(
                    text = "⌄",
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }

            DropdownMenu(
                expanded = accountMenuExpanded,
                onDismissRequest = { accountMenuExpanded = false },
            ) {
                accounts.forEach { a ->
                    val isActive = a.id == activeAccountId
                    DropdownMenuItem(
                        text = {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                horizontalArrangement = Arrangement.spacedBy(6.dp),
                            ) {
                                Text(
                                    if (isActive) "● " else "   ",
                                    color = if (isActive) MaterialTheme.colorScheme.primary else Color.Transparent,
                                    fontSize = 11.sp,
                                )
                                Column {
                                    Text(a.name, fontWeight = if (isActive) FontWeight.Bold else FontWeight.Normal, fontSize = 13.sp)
                                    Text(a.email, fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                            }
                        },
                        onClick = {
                            accountMenuExpanded = false
                            onSelectAccount(a.id)
                        },
                    )
                }
                HorizontalDivider(Modifier.padding(vertical = 4.dp))
                DropdownMenuItem(
                    text = { Text("Manage accounts…", fontSize = 13.sp) },
                    onClick = {
                        accountMenuExpanded = false
                        onManageAccounts()
                    },
                )
            }
        }

        Spacer(Modifier.height(6.dp))

        // --- Folders Section Header ---
        Row(
            Modifier
                .fillMaxWidth()
                .padding(horizontal = 14.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                "FOLDERS",
                fontWeight = FontWeight.Bold,
                fontSize = 11.sp,
                letterSpacing = 1.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        // --- Folders List ---
        LazyColumn(
            Modifier
                .fillMaxWidth()
                .weight(1f),
        ) {
            items(folders, key = { it.id }) { f ->
                val active = f.id == activeFolderId
                val depthIndent = (10 + f.depth() * 12).dp

                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 6.dp, vertical = 2.dp)
                        .clip(RoundedCornerShape(6.dp))
                        .clickable { onSelectFolder(f.id) }
                        .background(
                            if (active) MaterialTheme.colorScheme.primary.copy(alpha = 0.16f)
                            else Color.Transparent,
                        )
                        .padding(start = depthIndent, end = 10.dp, top = 6.dp, bottom = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    // Folder icon based on role
                    Text(
                        text = folderIcon(f.role),
                        fontSize = 14.sp,
                    )

                    // Folder name
                    Text(
                        text = f.displayName(),
                        fontWeight = if (f.unread > 0 || active) FontWeight.Bold else FontWeight.Normal,
                        fontSize = 13.sp,
                        color = if (active) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )

                    // Total message count (muted, QML idiom)
                    if (f.count > 0) {
                        Text(
                            text = "${f.count}",
                            fontSize = 11.sp,
                            color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.7f),
                        )
                    }

                    // Unread badge
                    if (f.unread > 0) {
                        Box(
                            Modifier
                                .clip(RoundedCornerShape(9.dp))
                                .background(
                                    if (active) MaterialTheme.colorScheme.primary
                                    else MaterialTheme.colorScheme.outline.copy(alpha = 0.8f),
                                )
                                .padding(horizontal = 7.dp, vertical = 1.dp),
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(
                                text = "${f.unread}",
                                color = if (active) Color.White else MaterialTheme.colorScheme.onSurface,
                                fontSize = 10.sp,
                                fontWeight = FontWeight.Bold,
                            )
                        }
                    }
                }
            }

            if (folders.isEmpty()) {
                item {
                    Text(
                        if (currentEmail.isBlank()) "Add an account to begin."
                        else "No folders yet — press ⟳ to sync.",
                        fontSize = 12.sp,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }
        }
    }
}
