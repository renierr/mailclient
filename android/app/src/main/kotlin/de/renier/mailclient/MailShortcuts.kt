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
import android.os.PersistableBundle
import android.util.Log
import de.renier.mailclient.ui.state.Account

// Launcher shortcuts (long-press on the app icon). Compose is static
// (xml/shortcuts.xml), so the menu has it from install on; this object
// keeps the dynamic ones, one inbox per account. Each opens MainActivity
// like a notification tap (MailNotifier.ACTION_OPEN), with a payload the
// shell resolves: "compose" or "inbox:<account id>". Republished when the
// account list loads and the shortcuts differ; a shortcut pinned to the
// home screen for a removed account is disabled instead of opening nothing.
object MailShortcuts {
    const val COMPOSE = "compose"
    const val INBOX_PREFIX = "inbox:"

    private const val TAG = "mailclient"
    private const val BRAND = 0xFF3B82F6.toInt()

    // Everything a shortcut shows, so an unchanged list is not republished:
    // the system rate-limits these calls.
    private const val SIGNATURE = "signature"

    // Icon canvas: adaptive icons are 108dp with a 72dp visible circle.
    private const val ICON_PX = 216

    fun update(context: Context, accounts: List<Account>) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.N_MR1) return
        val manager = context.getSystemService(ShortcutManager::class.java) ?: return
        try {
            // Launchers show about four, the static Compose among them
            // (it counts against the same limit); the rest would be cut off.
            val room = (manager.maxShortcutCountPerActivity - 1).coerceIn(0, 3)
            val inboxes = accounts.filter { it.id >= 0 }.take(room).mapIndexed { i, a ->
                val label = a.name.ifBlank { a.email }.ifBlank { "Inbox" }
                val longLabel = "Inbox · ${a.email.ifBlank { label }}"
                val signature = listOf(label, longLabel, a.initials, a.avatarLight).joinToString("|")
                ShortcutInfo.Builder(context, INBOX_PREFIX + a.id)
                    .setShortLabel(label)
                    .setLongLabel(longLabel)
                    .setIcon(accountIcon(a))
                    .setIntent(intent(context, INBOX_PREFIX + a.id))
                    .setRank(i)
                    .setExtras(PersistableBundle().apply { putString(SIGNATURE, signature) })
                    .build()
            }
            val published = manager.dynamicShortcuts.map { it.id to it.extras?.getString(SIGNATURE) }
            if (published != inboxes.map { it.id to it.extras?.getString(SIGNATURE) }) {
                // false: rate-limited (the app was in the background).
                if (!manager.setDynamicShortcuts(inboxes)) {
                    Log.w(TAG, "launcher shortcuts rate-limited, retried on the next account load")
                }
            }

            // Home-screen copies: a removed account's go grey, a re-added
            // id comes back.
            val live = accounts.map { INBOX_PREFIX + it.id }.toSet()
            val pinned = manager.pinnedShortcuts.map { it.id }.filter { it.startsWith(INBOX_PREFIX) }
            val (keep, gone) = pinned.partition { it in live }
            if (keep.isNotEmpty()) manager.enableShortcuts(keep)
            if (gone.isNotEmpty()) manager.disableShortcuts(gone, "Account removed")
        } catch (e: Exception) {
            // A launcher without shortcut support or a locked user: the app
            // works the same without them.
            Log.w(TAG, "launcher shortcuts not updated", e)
        }
    }

    // Single top like a notification tap: an open shell takes the payload
    // in onNewIntent instead of being rebuilt (which would drop a composer).
    private fun intent(context: Context, payload: String): Intent =
        Intent(context, MainActivity::class.java)
            .setAction(MailNotifier.ACTION_OPEN)
            .putExtra(MailNotifier.EXTRA_PAYLOAD, payload)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)

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
