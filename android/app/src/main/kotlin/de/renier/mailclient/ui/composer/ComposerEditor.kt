package de.renier.mailclient.ui.composer

import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.R

/**
 * The message body: formatting toolbar, the Markdown text field and a
 * preview of what the recipient will see, in one framed box. Formatting and
 * inline images sit in the text as marks and `![name](inline:N)` tokens;
 * the core renders them (`mailcore::compose::markdown`), for the preview
 * and on send alike.
 */
@Composable
fun ComposerEditor(
    value: TextFieldValue,
    onValueChange: (TextFieldValue) -> Unit,
    imagesJson: String,
    sendFormat: String,
    onAction: (String) -> Unit,
    onImage: () -> Unit,
    onAttach: () -> Unit,
) {
    var preview by remember { mutableStateOf(false) }
    val scheme = MaterialTheme.colorScheme
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(top = 12.dp)
            .border(1.dp, scheme.outlineVariant, RoundedCornerShape(8.dp)),
    ) {
        // Scrolls sideways rather than wrapping: seven 48dp targets fit a
        // 360dp phone, larger text scales may not.
        Row(
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 4.dp),
        ) {
            val editing = !preview
            IconButton(onClick = { onAction("bold") }, enabled = editing) {
                Text("B", fontWeight = FontWeight.Bold)
            }
            IconButton(onClick = { onAction("italic") }, enabled = editing) {
                Text("I", fontStyle = FontStyle.Italic)
            }
            IconButton(onClick = { onAction("quote") }, enabled = editing) {
                Text("“", style = MaterialTheme.typography.titleLarge)
            }
            IconButton(onClick = { onAction("bullet") }, enabled = editing) {
                Text("•", style = MaterialTheme.typography.titleLarge)
            }
            IconButton(onClick = onImage, enabled = editing) {
                Icon(painterResource(R.drawable.ic_image), "Insert image inline")
            }
            IconButton(onClick = onAttach) {
                Icon(painterResource(R.drawable.ic_attach), "Attach files")
            }
            IconToggleButton(checked = preview, onCheckedChange = { preview = it }) {
                Icon(
                    painterResource(if (preview) R.drawable.ic_edit else R.drawable.ic_mail),
                    if (preview) "Back to editing" else "Preview",
                )
            }
        }
        HorizontalDivider(color = scheme.outlineVariant)
        if (preview) {
            val html = remember(value.text, imagesJson) {
                runCatching { MailNative.composePreviewHtml(value.text, imagesJson) }.getOrDefault("")
            }
            Box(modifier = Modifier.fillMaxWidth().height(260.dp), contentAlignment = Alignment.Center) {
                if (value.text.isBlank()) {
                    Text("Nothing to preview yet", style = MaterialTheme.typography.bodySmall)
                } else {
                    HtmlPreview(html, Modifier.fillMaxWidth().height(260.dp))
                }
            }
        } else {
            Box(modifier = Modifier.padding(horizontal = 12.dp, vertical = 12.dp)) {
                if (value.text.isEmpty()) {
                    Text("Write your message", color = scheme.onSurfaceVariant)
                }
                BasicTextField(
                    value = value,
                    onValueChange = onValueChange,
                    textStyle = MaterialTheme.typography.bodyLarge.copy(color = scheme.onSurface),
                    cursorBrush = SolidColor(scheme.primary),
                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                    modifier = Modifier.fillMaxWidth().heightIn(min = 220.dp),
                )
            }
        }
        HorizontalDivider(color = scheme.outlineVariant)
        val note = remember(sendFormat, value.text) {
            runCatching { MailNative.composeFormatNote(sendFormat, value.text) }.getOrDefault("")
        }
        Text(
            "$note · Markdown: **bold**, *italic*, > quote",
            style = MaterialTheme.typography.bodySmall,
            color = scheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = 12.dp, vertical = 6.dp),
        )
    }
}
