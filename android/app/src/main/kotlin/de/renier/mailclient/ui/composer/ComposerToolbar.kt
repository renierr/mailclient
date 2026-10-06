package de.renier.mailclient.ui.composer

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import de.renier.mailclient.R

/**
 * The formatting bar, Qt's ComposerToolbar: bold, italic, underline, list,
 * quote, link, clear formatting, inline image, attach, HTML source. Pinned
 * above the keyboard; scrolls sideways where a narrow phone or a large text
 * scale cannot fit every 48dp target. Buttons light up for the formatting
 * at the caret.
 */
@Composable
fun ComposerToolbar(
    format: FormatState,
    sourceMode: Boolean,
    onExec: (String) -> Unit,
    onQuote: () -> Unit,
    onLink: () -> Unit,
    onImage: () -> Unit,
    onAttach: () -> Unit,
    onToggleSource: () -> Unit,
) {
    val editing = !sourceMode
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 4.dp),
    ) {
        Toggle(format.bold, editing, "Bold", { onExec("bold") }) { Text("B", fontWeight = FontWeight.Bold) }
        Toggle(format.italic, editing, "Italic", { onExec("italic") }) { Text("I", fontStyle = FontStyle.Italic) }
        Toggle(format.underline, editing, "Underline", { onExec("underline") }) {
            Text("U", textDecoration = TextDecoration.Underline)
        }
        Separator()
        Toggle(format.list, editing, "Bullet list", { onExec("insertUnorderedList") }) {
            Icon(painterResource(R.drawable.ic_format_list), null)
        }
        Toggle(format.quote, editing, "Quote", onQuote) {
            Icon(painterResource(R.drawable.ic_format_quote), null)
        }
        IconButton(onClick = onLink, enabled = editing) {
            Icon(painterResource(R.drawable.ic_link), "Insert link")
        }
        IconButton(onClick = { onExec("removeFormat") }, enabled = editing) {
            Icon(painterResource(R.drawable.ic_format_clear), "Clear formatting")
        }
        Separator()
        IconButton(onClick = onImage, enabled = editing) {
            Icon(painterResource(R.drawable.ic_image), "Insert image inline")
        }
        IconButton(onClick = onAttach) {
            Icon(painterResource(R.drawable.ic_attach), "Attach files")
        }
        Toggle(sourceMode, true, if (sourceMode) "Back to formatted text" else "HTML source", onToggleSource) {
            Icon(painterResource(R.drawable.ic_code), null)
        }
    }
}

@Composable
private fun Toggle(
    checked: Boolean,
    enabled: Boolean,
    label: String,
    onClick: () -> Unit,
    content: @Composable () -> Unit,
) {
    IconToggleButton(
        checked = checked,
        onCheckedChange = { onClick() },
        enabled = enabled,
        modifier = Modifier.semantics { contentDescription = label },
    ) { content() }
}

@Composable
private fun Separator() {
    VerticalDivider(
        color = MaterialTheme.colorScheme.outlineVariant,
        modifier = Modifier.padding(horizontal = 4.dp).height(24.dp),
    )
}
