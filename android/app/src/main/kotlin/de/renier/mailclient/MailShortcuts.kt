package de.renier.mailclient

import android.content.Context
import android.content.Intent
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.graphics.drawable.Icon
import android.os.Build
import android.util.Log
import de.renier.mailclient.ui.state.Account

// Launcher shortcuts (long-press on the app icon): Compose, then one inbox
// per account. Each opens MainActivity like a notification tap
// (MailNotifier.ACTION_OPEN), with a payload the shell resolves:
// "compose" or "inbox:<account id>". Rebuilt whenever the account list
// loads; a shortcut pinned to the home screen for a removed account is
// disabled instead of opening nothing.
object MailShortcuts {
    const val COMPOSE = "compose"
    const val INBOX_PREFIX = "inbox:"

    private const val TAG = "mailclient"
    private const val BRAND = 0xFF3B82F6.toInt()

    // Icon canvas: adaptive icons are 108dp with a 72dp visible circle.
    private const val ICON_PX = 216

    fun update(context: Context, accounts: List<Account>) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.N_MR1) return
        val manager = context.getSystemService(ShortcutManager::class.java) ?: return
        try {
            val compose = ShortcutInfo.Builder(context, COMPOSE)
                .setShortLabel("Compose")
                .setLongLabel("Compose new mail")
                .setIcon(composeIcon(context))
                .setIntent(intent(context, COMPOSE))
                .setRank(0)
                .build()
            // Launchers show about four; the rest would only be cut off.
            val room = (manager.maxShortcutCountPerActivity - 1).coerceIn(0, 3)
            val inboxes = accounts.filter { it.id >= 0 }.take(room).mapIndexed { i, a ->
                val label = a.name.ifBlank { a.email }.ifBlank { "Inbox" }
                ShortcutInfo.Builder(context, INBOX_PREFIX + a.id)
                    .setShortLabel(label)
                    .setLongLabel("Inbox · ${a.email.ifBlank { label }}")
                    .setIcon(accountIcon(a))
                    .setIntent(intent(context, INBOX_PREFIX + a.id))
                    .setRank(i + 1)
                    .build()
            }
            manager.dynamicShortcuts = listOf(compose) + inboxes

            // Home-screen copies: a removed account's go grey, a re-added
            // id comes back.
            val live = accounts.map { INBOX_PREFIX + it.id }.toSet()
            val pinned = manager.pinnedShortcuts.map { it.id }.filter { it.startsWith(INBOX_PREFIX) }
            val (keep, gone) = pinned.partition { it in live }
            if (keep.isNotEmpty()) manager.enableShortcuts(keep)
            if (gone.isNotEmpty()) manager.disableShortcuts(gone, "Account removed")
        } catch (e: Exception) {
            // Rate limits or a launcher without shortcut support: the app
            // works the same without them.
            Log.w(TAG, "launcher shortcuts not updated: ${e.message}")
        }
    }

    // Single top like a notification tap: an open shell takes the payload
    // in onNewIntent instead of being rebuilt (which would drop a composer).
    private fun intent(context: Context, payload: String): Intent =
        Intent(context, MainActivity::class.java)
            .setAction(MailNotifier.ACTION_OPEN)
            .putExtra(MailNotifier.EXTRA_PAYLOAD, payload)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)

    private fun composeIcon(context: Context): Icon {
        val bitmap = canvasBitmap(BRAND) { canvas ->
            val glyph = context.getDrawable(R.drawable.ic_edit)?.mutate() ?: return@canvasBitmap
            glyph.setTint(0xFFFFFFFF.toInt())
            val size = ICON_PX * 3 / 8
            val inset = (ICON_PX - size) / 2
            glyph.setBounds(inset, inset, inset + size, inset + size)
            glyph.draw(canvas)
        }
        return wrap(bitmap)
    }

    // The account's avatar (core-decided initials and colour), light theme:
    // launchers do not follow the app's theme.
    private fun accountIcon(account: Account): Icon {
        val bg = runCatching { android.graphics.Color.parseColor(account.avatarLight) }.getOrDefault(BRAND)
        val bitmap = canvasBitmap(bg) { canvas ->
            val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
                color = 0xFFFFFFFF.toInt()
                textAlign = Paint.Align.CENTER
                textSize = ICON_PX * 0.24f
                typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
            }
            val y = ICON_PX / 2f - (paint.descent() + paint.ascent()) / 2f
            canvas.drawText(account.initials.take(2), ICON_PX / 2f, y, paint)
        }
        return wrap(bitmap)
    }

    private fun canvasBitmap(background: Int, draw: (Canvas) -> Unit): Bitmap {
        val bitmap = Bitmap.createBitmap(ICON_PX, ICON_PX, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            // Adaptive: full bleed, the launcher masks it.
            canvas.drawColor(background)
        } else {
            val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = background }
            canvas.drawCircle(ICON_PX / 2f, ICON_PX / 2f, ICON_PX / 2f, paint)
        }
        draw(canvas)
        return bitmap
    }

    private fun wrap(bitmap: Bitmap): Icon =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Icon.createWithAdaptiveBitmap(bitmap)
        } else {
            Icon.createWithBitmap(bitmap)
        }
}
