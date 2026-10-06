package de.renier.mailclient.ui.composer

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LocalTextStyle
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.PopupProperties
import de.renier.mailclient.MailNative
import de.renier.mailclient.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import org.json.JSONArray

// The composer's building blocks, Flutter's composer_widgets in Compose.
// Labels sit above their fields (AGENTS.md narrow rule), so nothing shares a
// row with a text field at 360dp.

/** A labelled header line: label and toggles above, the field below. */
@Composable
fun ComposerHeaderRow(
    label: String,
    trailing: @Composable () -> Unit = {},
    content: @Composable () -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth().padding(top = 8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                label,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
            trailing()
        }
        content()
    }
}

/** Cc / Bcc / Reply-To switch beside a header label. */
@Composable
fun ComposerToggle(label: String, active: Boolean, onClick: () -> Unit) {
    FilterChip(
        selected = active,
        onClick = onClick,
        label = { Text(label) },
        modifier = Modifier.padding(start = 6.dp),
    )
}

/** A plain one-line field with the composer's look. */
@Composable
fun ComposerTextField(
    value: TextFieldValue,
    onValueChange: (TextFieldValue) -> Unit,
    placeholder: String = "",
    keyboard: KeyboardType = KeyboardType.Text,
    suffix: String? = null,
    // The From address sits right against its locked domain.
    alignEnd: Boolean = false,
    modifier: Modifier = Modifier,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        singleLine = true,
        textStyle = if (alignEnd) LocalTextStyle.current.copy(textAlign = TextAlign.End) else LocalTextStyle.current,
        placeholder = if (placeholder.isEmpty()) null else ({ Text(placeholder) }),
        suffix = suffix?.let { { Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant) } },
        keyboardOptions = KeyboardOptions(keyboardType = keyboard, imeAction = ImeAction.Next),
        modifier = modifier.fillMaxWidth(),
    )
}

private data class Suggestion(val address: String, val label: String)

/**
 * A recipient line with suggestions from known contacts. Only the segment
 * being typed is completed (the core finds and replaces it), so a
 * half-typed list is never clobbered.
 */
@Composable
fun RecipientField(
    value: TextFieldValue,
    onValueChange: (TextFieldValue) -> Unit,
    suggest: Boolean,
    placeholder: String = "",
) {
    var focused by remember { mutableStateOf(false) }
    var suggestions by remember { mutableStateOf(emptyList<Suggestion>()) }
    val text = value.text
    LaunchedEffect(text, focused, suggest) {
        if (!suggest || !focused) {
            suggestions = emptyList()
            return@LaunchedEffect
        }
        delay(150)
        suggestions = withContext(Dispatchers.IO) {
            runCatching {
                val segment = MailNative.recipientSegment(text)
                if (segment.isBlank()) return@runCatching emptyList()
                val arr = JSONArray(MailNative.contactsJson(segment))
                List(arr.length()) { i ->
                    val c = arr.getJSONObject(i)
                    val address = c.optString("address")
                    val name = c.optString("alias").takeIf { it.isNotEmpty() && it != "null" }
                        ?: c.optString("name").takeIf { it.isNotEmpty() && it != "null" }
                    Suggestion(address, if (name != null) "$name <$address>" else address)
                }.filter { it.address.isNotEmpty() }
            }.getOrDefault(emptyList())
        }
    }
    Box {
        OutlinedTextField(
            value = value,
            onValueChange = onValueChange,
            singleLine = true,
            placeholder = if (placeholder.isEmpty()) null else ({ Text(placeholder) }),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email, imeAction = ImeAction.Next),
            modifier = Modifier.fillMaxWidth().onFocusChanged { focused = it.isFocused },
        )
        DropdownMenu(
            expanded = suggestions.isNotEmpty(),
            onDismissRequest = { suggestions = emptyList() },
            // Typing goes on in the field while the list is open.
            properties = PopupProperties(focusable = false),
        ) {
            for (s in suggestions) {
                DropdownMenuItem(
                    text = { Text(s.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    onClick = {
                        val next = MailNative.replaceRecipientSegment(text, s.address)
                        onValueChange(TextFieldValue(next, TextRange(next.length)))
                        suggestions = emptyList()
                    },
                )
            }
        }
    }
}

/** A tinted callout: danger for a reply-to mismatch, neutral otherwise. */
@Composable
fun ComposerNotice(text: String, danger: Boolean = false) {
    val scheme = MaterialTheme.colorScheme
    Surface(
        color = if (danger) scheme.errorContainer else scheme.surfaceContainerHighest,
        shape = RoundedCornerShape(8.dp),
        modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
    ) {
        Text(
            text,
            style = MaterialTheme.typography.bodySmall,
            color = if (danger) scheme.onErrorContainer else scheme.onSurface,
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
        )
    }
}

/**
 * Picked files as chips; empty, it takes no room. Only the chip's X
 * removes a file, and only after a confirm: a stray tap on the name must
 * not drop an attachment unnoticed.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AttachmentTray(files: List<PickedFile>, onRemove: (PickedFile) -> Unit) {
    if (files.isEmpty()) return
    var confirm by remember { mutableStateOf<PickedFile?>(null) }
    val scheme = MaterialTheme.colorScheme
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
    ) {
        for (f in files) {
            Surface(
                shape = RoundedCornerShape(8.dp),
                border = BorderStroke(1.dp, scheme.outlineVariant),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(start = 10.dp)) {
                    Icon(painterResource(R.drawable.ic_attach), null, Modifier.size(16.dp))
                    Text(
                        f.name,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.labelLarge,
                        modifier = Modifier.padding(start = 6.dp).widthIn(max = 220.dp),
                    )
                    IconButton(onClick = { confirm = f }) {
                        Icon(painterResource(R.drawable.ic_close), "Remove ${f.name}", Modifier.size(18.dp))
                    }
                }
            }
        }
    }
    confirm?.let { f ->
        AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text("Remove attachment?") },
            text = { Text(f.name) },
            confirmButton = {
                Button(onClick = {
                    confirm = null
                    onRemove(f)
                }) { Text("Remove") }
            },
            dismissButton = { TextButton(onClick = { confirm = null }) { Text("Cancel") } },
        )
    }
}
