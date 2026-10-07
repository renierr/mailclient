package de.renier.mailclient.ui.list

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import de.renier.mailclient.ui.common.FormDialog
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.applyDatePreset
import de.renier.mailclient.ui.state.clearDateFilter
import de.renier.mailclient.ui.state.clearListFilters
import de.renier.mailclient.ui.state.hasDateFilter
import de.renier.mailclient.ui.state.hasListFilter
import de.renier.mailclient.ui.state.setAfterDay
import de.renier.mailclient.ui.state.setAttachmentsOnly
import de.renier.mailclient.ui.state.setBeforeDay
import de.renier.mailclient.ui.state.setSort
import de.renier.mailclient.ui.state.setStarredOnly
import de.renier.mailclient.ui.state.setUnreadOnly

// Sort menu (Qt sortMenu, Flutter sort PopupMenuButton): newest/oldest,
// From A–Z/Z–A, Subject A–Z/Z–A, with a tick on the active order. Hidden
// while searching — hits stay newest-first there.
@Composable
fun SortMenu(state: MailState, onDismiss: () -> Unit) {
    DropdownMenu(expanded = true, onDismissRequest = onDismiss) {
        sortEntry(state, "Date, newest first", "date", true, onDismiss)
        sortEntry(state, "Date, oldest first", "date", false, onDismiss)
        sortEntry(state, "From, A–Z", "from", false, onDismiss)
        sortEntry(state, "From, Z–A", "from", true, onDismiss)
        sortEntry(state, "Subject, A–Z", "subject", false, onDismiss)
        sortEntry(state, "Subject, Z–A", "subject", true, onDismiss)
    }
}

@Composable
private fun sortEntry(
    state: MailState,
    label: String,
    field: String,
    descending: Boolean,
    onDismiss: () -> Unit,
) {
    val active = state.sortField == field && state.sortDesc == descending
    DropdownMenuItem(
        text = { Text((if (active) "✓ " else "") + label) },
        onClick = {
            onDismiss()
            if (!active) state.setSort(field, descending)
        },
    )
}

/** Short sort name for the header tooltip. */
fun sortShortLabel(state: MailState): String = when (state.sortField) {
    "from" -> "From"
    "subject" -> "Subject"
    else -> "Date"
}

// Filter menu (Qt filterMenu, Flutter filter PopupMenuButton): unread /
// starred / attachments toggles plus date presets and a custom range,
// AND-combined. Applies to loaded rows and search hits alike.
@Composable
fun FilterMenu(state: MailState, onCustomRange: () -> Unit, onDismiss: () -> Unit) {
    DropdownMenu(expanded = true, onDismissRequest = onDismiss) {
        filterToggle("Unread only", state.filterUnread, { state.setUnreadOnly(it) })
        filterToggle("Starred only", state.filterStarred, { state.setStarredOnly(it) })
        filterToggle("With attachments", state.filterAttachments, { state.setAttachmentsOnly(it) })
        HorizontalDivider()
        DropdownMenuItem(
            text = { Text("Today") },
            onClick = {
                onDismiss()
                state.applyDatePreset("today")
            },
        )
        DropdownMenuItem(
            text = { Text("Last 7 days") },
            onClick = {
                onDismiss()
                state.applyDatePreset("week")
            },
        )
        DropdownMenuItem(
            text = { Text("Last 30 days") },
            onClick = {
                onDismiss()
                state.applyDatePreset("month")
            },
        )
        DropdownMenuItem(
            text = { Text("Older than 30 days") },
            onClick = {
                onDismiss()
                state.applyDatePreset("older_month")
            },
        )
        DropdownMenuItem(text = { Text("Custom range…") }, onClick = onCustomRange)
        if (state.hasDateFilter) {
            DropdownMenuItem(
                text = { Text("Clear dates") },
                onClick = {
                    onDismiss()
                    state.clearDateFilter()
                },
            )
        }
        if (state.hasListFilter) {
            HorizontalDivider()
            DropdownMenuItem(
                text = { Text("Clear filters") },
                onClick = {
                    onDismiss()
                    state.clearListFilters()
                },
            )
        }
    }
}

@Composable
private fun filterToggle(label: String, on: Boolean, set: (Boolean) -> Unit) {
    DropdownMenuItem(
        text = { Text((if (on) "✓ " else "") + label) },
        onClick = { set(!on) },
    )
}

// Custom date range (Qt dateDialog): After inclusive / Before exclusive
// YYYY-MM-DD. Rejects anything the core could not read as a day pair.
@Composable
fun DateRangeDialog(state: MailState, onDismiss: () -> Unit) {
    var after by remember { mutableStateOf(state.filterAfter) }
    var before by remember { mutableStateOf(state.filterBefore) }
    var error by remember { mutableStateOf<String?>(null) }
    val day = Regex("""\d{4}-\d{2}-\d{2}""")
    fun apply() {
        val bad = (after.isNotEmpty() && !day.matches(after)) ||
            (before.isNotEmpty() && !day.matches(before))
        if (after.isEmpty() && before.isEmpty() || bad) {
            error = "Enter at least one date as YYYY-MM-DD."
            return
        }
        if (after.isNotEmpty() && before.isNotEmpty() && after > before) {
            error = "After must not be later than Before."
            return
        }
        state.setAfterDay(after)
        state.setBeforeDay(before)
        onDismiss()
    }
    FormDialog(title = "Custom date range", confirmLabel = "Apply", onConfirm = ::apply, onDismiss = onDismiss) {
        OutlinedTextField(
            value = after,
            onValueChange = { after = it.trim(); error = null },
            label = { Text("After (YYYY-MM-DD)") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = before,
            onValueChange = { before = it.trim(); error = null },
            label = { Text("Before (YYYY-MM-DD)") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
        )
        error?.let { Text(it, modifier = Modifier.padding(top = 8.dp)) }
    }
}

// Destructive-action confirm (trash per the delete preference, purge
// always) lives in ui.common now, shared with the reader and composer.
