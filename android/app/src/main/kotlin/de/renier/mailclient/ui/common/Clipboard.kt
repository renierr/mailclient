package de.renier.mailclient.ui.common

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context

/** Put [text] on the system clipboard under [label]. */
fun copyToClipboard(context: Context, label: String, text: String) {
    (context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager)
        ?.setPrimaryClip(ClipData.newPlainText(label, text))
}
