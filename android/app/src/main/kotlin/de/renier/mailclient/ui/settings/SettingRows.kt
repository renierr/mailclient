package de.renier.mailclient.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

// The settings form's rows. Labels sit above their controls, so nothing
// shares a line with a dropdown at 360dp or a large text scale.

/** Small muted text under a setting. */
@Composable
fun SettingHint(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = modifier,
    )
}

@Composable
fun SettingHeading(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.titleMedium,
        modifier = Modifier.padding(top = 16.dp, bottom = 4.dp),
    )
}

/** A pick-one setting: title (and hint) above a dropdown of [options]. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingChoice(
    title: String,
    value: String,
    options: List<String>,
    label: (String) -> String,
    onChange: (String) -> Unit,
    help: String? = null,
    enabled: Boolean = true,
) {
    var open by remember { mutableStateOf(false) }
    val shown = if (value in options) value else options.firstOrNull().orEmpty()
    Column(modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Text(title, style = MaterialTheme.typography.bodyLarge)
        help?.let { SettingHint(it) }
        ExposedDropdownMenuBox(
            expanded = open && enabled,
            onExpandedChange = { if (enabled) open = it },
            modifier = Modifier.padding(top = 4.dp),
        ) {
            OutlinedTextField(
                value = label(shown),
                onValueChange = {},
                readOnly = true,
                enabled = enabled,
                singleLine = true,
                trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = open) },
                modifier = Modifier.fillMaxWidth().menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable, enabled),
            )
            ExposedDropdownMenu(expanded = open && enabled, onDismissRequest = { open = false }) {
                for (o in options) {
                    DropdownMenuItem(
                        text = { Text(label(o), maxLines = 2, overflow = TextOverflow.Ellipsis) },
                        onClick = {
                            open = false
                            onChange(o)
                        },
                    )
                }
            }
        }
    }
}

/** An on/off setting: title (and hint) with a switch at the end. */
@Composable
fun SettingSwitch(title: String, checked: Boolean, onChange: (Boolean) -> Unit, help: String? = null) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            help?.let { SettingHint(it) }
        }
        Switch(checked = checked, onCheckedChange = onChange)
    }
}

/** One status line: an icon and text, in the error colour when it is one. */
@Composable
fun StatusLine(icon: Int, text: String, error: Boolean = false) {
    val color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant
    Row(modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp)) {
        Icon(painterResource(icon), null, tint = color, modifier = Modifier.size(18.dp))
        Text(
            text,
            style = MaterialTheme.typography.bodyMedium,
            color = if (error) color else MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.padding(start = 8.dp),
        )
    }
}
