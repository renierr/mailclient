package de.renier.mailclient

import android.app.Activity
import android.app.AlertDialog
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.DocumentsContract
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.WindowInsets
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import androidx.core.content.FileProvider
import org.json.JSONArray
import org.json.JSONObject

// Experiment (branch `experiment/native-reader`): the Flutter reader's
// feature set over a native scroll. The point under test is the *combined*
// case — the full header with every button and menu scrolling together with
// the body in one parent ScrollView — so nothing is pinned: not even Back,
// which also lives in the scrolling header like the Flutter reader's
// `onClose` (system Back still finishes).
//
// Read path and every mutation go over JNI to the same core and net thread
// Dart uses. Composer, find-similar and the move picker go back to Flutter
// (see delegateToFlutter): they are shell flows, not reader scrolling.
class ReaderActivity : Activity() {
    companion object {
        const val EXTRA_ACCOUNT_ID = "accountId"
        const val EXTRA_FOLDER_ID = "folderId"
        const val EXTRA_UID = "uid"
        const val EXTRA_AUTO_MARK_READ = "autoMarkRead"
        const val EXTRA_MARK_READ_DELAY = "markReadDelaySecs"
        const val EXTRA_LOAD_REMOTE = "loadRemoteImages"
        const val EXTRA_LINK_ACTION = "linkClickAction"
        const val EXTRA_READER_SCALE = "readerScale"
        const val EXTRA_DELETE_PERMANENT = "deleteIsPermanent"
        const val EXTRA_CONFIRM_DELETE = "confirmDelete"

        private const val REQ_SAVE_ONE = 11
        private const val REQ_SAVE_ALL = 12
        private const val REQ_SAVE_EML = 13

        fun openIntent(context: Context, args: Map<String, Any?>): Intent =
            Intent(context, ReaderActivity::class.java).apply {
                putExtra(EXTRA_ACCOUNT_ID, (args["accountId"] as? Number)?.toLong() ?: -1L)
                putExtra(EXTRA_FOLDER_ID, (args["folderId"] as? Number)?.toLong() ?: -1L)
                putExtra(EXTRA_UID, (args["uid"] as? Number)?.toInt() ?: -1)
                putExtra(EXTRA_AUTO_MARK_READ, args["autoMarkRead"] as? Boolean ?: true)
                putExtra(EXTRA_MARK_READ_DELAY, (args["markReadDelaySecs"] as? Number)?.toLong() ?: 0L)
                putExtra(EXTRA_LOAD_REMOTE, args["loadRemoteImages"] as? Boolean ?: false)
                putExtra(EXTRA_LINK_ACTION, args["linkClickAction"] as? String ?: "examine")
                putExtra(EXTRA_READER_SCALE, (args["readerScale"] as? Number)?.toFloat() ?: 1f)
                putExtra(EXTRA_DELETE_PERMANENT, args["deleteIsPermanent"] as? Boolean ?: false)
                putExtra(EXTRA_CONFIRM_DELETE, args["confirmDelete"] as? Boolean ?: true)
            }
    }

    private var accountId = -1L
    private var folderId = -1L
    private var uid = -1
    private var autoMarkRead = true
    private var markReadDelaySecs = 0L
    private var loadRemoteImages = false
    private var linkClickAction = "examine"
    private var readerScale = 1f
    private var deleteIsPermanent = false
    private var confirmDeleteSetting = true

    private var msg: JSONObject? = null
    private var headers: JSONObject? = null
    private var dark = false
    private var detailsExpanded = false
    private var remoteOnce = false
    private var originalColors = false
    private var gone = false
    private var mutated = false

    private val ui = Handler(Looper.getMainLooper())
    private var markReadTask: Runnable? = null

    private lateinit var content: LinearLayout
    private lateinit var progress: ProgressBar
    private lateinit var errorView: TextView
    private lateinit var undoBar: LinearLayout
    private lateinit var undoLabel: TextView
    private var undoBatch: String? = null

    private var pendingSaveName = ""
    private var pendingSaveMime = ""
    private var pendingSaveBytes: ByteArray? = null
    private var pendingSaveAll: List<PendingFile> = emptyList()
    private var pendingEmlName = ""
    private var pendingEmlBytes: ByteArray? = null

    private data class PendingFile(val name: String, val mime: String, val bytes: ByteArray)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        accountId = intent.getLongExtra(EXTRA_ACCOUNT_ID, -1)
        folderId = intent.getLongExtra(EXTRA_FOLDER_ID, -1)
        uid = intent.getIntExtra(EXTRA_UID, -1)
        if (accountId < 0 || folderId < 0 || uid < 0) {
            finish()
            return
        }
        autoMarkRead = intent.getBooleanExtra(EXTRA_AUTO_MARK_READ, true)
        markReadDelaySecs = intent.getLongExtra(EXTRA_MARK_READ_DELAY, 0)
        loadRemoteImages = intent.getBooleanExtra(EXTRA_LOAD_REMOTE, false)
        linkClickAction = intent.getStringExtra(EXTRA_LINK_ACTION) ?: "examine"
        readerScale = intent.getFloatExtra(EXTRA_READER_SCALE, 1f)
        deleteIsPermanent = intent.getBooleanExtra(EXTRA_DELETE_PERMANENT, false)
        confirmDeleteSetting = intent.getBooleanExtra(EXTRA_CONFIRM_DELETE, true)
        dark = (resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
            Configuration.UI_MODE_NIGHT_YES

        val root = FrameLayout(this).apply {
            layoutParams = FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT,
            )
            // Edge-to-edge (enforced since Android 15): keep the status bar
            // off the header instead of drawing the mail under it.
            setOnApplyWindowInsetsListener { v, insets ->
                val top = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    insets.getInsets(WindowInsets.Type.systemBars()).top
                } else {
                    @Suppress("DEPRECATION")
                    insets.systemWindowInsetTop
                }
                v.setPadding(0, top, 0, 0)
                insets
            }
        }
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.MATCH_PARENT,
            )
        }
        progress = ProgressBar(this).apply {
            isIndeterminate = true
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT,
            ).apply { gravity = Gravity.CENTER_HORIZONTAL }
        }
        column.addView(progress)
        errorView = TextView(this).apply {
            visibility = View.GONE
            setPadding(dp(16), dp(16), dp(16), dp(16))
        }
        column.addView(errorView)
        val scroll = ScrollView(this).apply {
            isFillViewport = true
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                0,
                1f,
            )
        }
        content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(12), dp(16), dp(32))
        }
        scroll.addView(content)
        column.addView(scroll)
        root.addView(column)

        undoBar = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            visibility = View.GONE
            setPadding(dp(16), dp(10), dp(16), dp(10))
            setBackgroundColor(attrColor(android.R.attr.colorBackground, if (dark) 0xFF2B2B2B.toInt() else 0xFFFFFFFF.toInt()))
            elevation = dp(6).toFloat()
            layoutParams = FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT,
                FrameLayout.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM,
            )
        }
        undoLabel = TextView(this).apply {
            layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
        }
        val undoBtn = Button(this).apply {
            text = "Undo"
            setOnClickListener { undoQueued() }
        }
        undoBar.addView(undoLabel)
        undoBar.addView(undoBtn)
        root.addView(undoBar)

        setContentView(root)
        load()
    }

    override fun onDestroy() {
        markReadTask?.let { ui.removeCallbacks(it) }
        super.onDestroy()
    }

    override fun finish() {
        if (mutated) MainActivity.readerDirty = true
        super.finish()
    }

    // -- data ---------------------------------------------------------------

    private fun bg(work: () -> Unit) {
        Thread {
            try {
                work()
            } catch (e: Exception) {
                runOnUiThread { toast("Error: ${e.message}") }
            }
        }.start()
    }

    private fun toast(text: String) {
        Toast.makeText(this, text, Toast.LENGTH_SHORT).show()
    }

    private fun load() {
        bg {
            MailNative.ensureInit(this)
            val m = JSONObject(MailNative.readerMessage(folderId, uid))
            val h = JSONObject(MailNative.readerHeaders(folderId, uid))
            runOnUiThread {
                msg = m
                headers = h
                progress.visibility = View.GONE
                render()
                planMarkRead(m.optBoolean("unread"))
            }
        }
    }

    private fun reload() {
        bg {
            val m = JSONObject(MailNative.readerMessage(folderId, uid))
            val h = JSONObject(MailNative.readerHeaders(folderId, uid))
            runOnUiThread {
                msg = m
                headers = h
                render()
            }
        }
    }

    private fun planMarkRead(unread: Boolean) {
        if (!unread) return
        bg {
            val plan = JSONObject(MailNative.markReadPlan(autoMarkRead, markReadDelaySecs, true))
            runOnUiThread {
                when (plan.optString("plan")) {
                    "now" -> applyRead()
                    "after" -> {
                        val delay = plan.optLong("delay_secs", 0).coerceAtLeast(0)
                        val task = Runnable {
                            markReadTask = null
                            if (!isFinishing && !isDestroyed) applyRead()
                        }
                        markReadTask = task
                        ui.postDelayed(task, delay * 1000)
                    }
                    else -> Unit
                }
            }
        }
    }

    private fun applyRead() {
        bg {
            MailNative.setReadFlag(accountId, folderId, uid, true)
            mutated = true
        }
    }

    private fun currentPaint(): String {
        val m = msg ?: return "theme"
        val colored = m.optBoolean("html_colored")
        if (!colored) return "theme"
        return if (dark && !originalColors) "darkened" else "original"
    }

    private fun themeColors(): IntArray =
        if (dark) intArrayOf(0x1C1B1F, 0xE6E1E5, 0x7AB4FF, 0xCAC4D0, 0x49454F)
        else intArrayOf(0xFFFFFF, 0x202124, 0x1A5FD0, 0x5F6368, 0xD0D4DA)

    private fun buildDoc(bodyHtml: String): String {
        val t = themeColors()
        return MailNative.readerDocument(
            bodyHtml, currentPaint(),
            t[0], t[1], t[2], t[3], t[4],
            loadRemoteImages || remoteOnce,
            readerScale * resources.configuration.fontScale,
            true,
        )
    }

    // -- render ---------------------------------------------------------------

    private fun render() {
        val m = msg ?: return
        val h = headers ?: JSONObject()
        content.removeAllViews()

        // Back scrolls with the header, like the Flutter reader's onClose.
        val topRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        val back = Button(this, null, android.R.attr.borderlessButtonStyle).apply {
            text = "‹ Back"
            setOnClickListener { finish() }
        }
        val subject = TextView(this).apply {
            text = m.optString("subject", "(no subject)")
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 20f)
            setTypeface(typeface, Typeface.BOLD)
            layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
        }
        topRow.addView(back)
        topRow.addView(subject)
        content.addView(topRow)

        content.addView(View(this).apply { layoutParams = LinearLayout.LayoutParams(1, dp(12)) })

        val senderRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        val initials = m.optString("initials", "?").ifEmpty { "?" }
        val avatarCss = if (dark) m.optString("avatar_dark") else m.optString("avatar_light")
        val avatar = TextView(this).apply {
            text = initials
            gravity = Gravity.CENTER
            setTextColor(0xFFFFFFFF.toInt())
            setTypeface(typeface, Typeface.BOLD)
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(parseCss(avatarCss, if (dark) 0xFF5F6368.toInt() else 0xFF80868B.toInt()))
            }
            layoutParams = LinearLayout.LayoutParams(dp(40), dp(40))
        }
        senderRow.addView(avatar)
        senderRow.addView(View(this).apply { layoutParams = LinearLayout.LayoutParams(dp(12), 1) })
        val who = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
        }
        val fromName = m.optString("from_name")
        val fromAddr = m.optString("from")
        who.addView(TextView(this).apply {
            text = if (fromName.isNotEmpty()) fromName else fromAddr
            setTypeface(typeface, Typeface.BOLD)
        })
        if (fromName.isNotEmpty()) who.addView(TextView(this).apply {
            text = fromAddr
            setTextColor(mutedColor())
            textSize = 13f
        })
        val toLine = m.optString("to").ifEmpty { h.optString("to") }
        if (toLine.isNotEmpty() && !detailsExpanded) who.addView(TextView(this).apply {
            text = "To $toLine"
            setTextColor(mutedColor())
            textSize = 13f
            maxLines = 1
        })
        val replyTo = m.optString("reply_to")
        if (m.optBoolean("reply_to_differs") && replyTo.isNotEmpty()) who.addView(TextView(this).apply {
            text = "Replies go to $replyTo, not to the sender"
            setTextColor(0xFFB3261E.toInt())
            textSize = 13f
        })
        senderRow.addView(who)
        val dateView = TextView(this).apply {
            text = h.optString("date").ifEmpty { m.optString("date") }
            setTextColor(mutedColor())
            textSize = 13f
        }
        senderRow.addView(dateView)
        val detailsBtn = Button(this, null, android.R.attr.borderlessButtonStyle).apply {
            text = if (detailsExpanded) "▾" else "▸"
            contentDescription = if (detailsExpanded) "Hide details" else "Show details"
            setOnClickListener {
                detailsExpanded = !detailsExpanded
                render()
            }
        }
        senderRow.addView(detailsBtn)
        content.addView(senderRow)

        if (detailsExpanded) {
            val fullFrom = h.optString("from").ifEmpty { fromAddr }
            val rows = listOf(
                "From" to fullFrom,
                "To" to h.optString("to"),
                "Cc" to h.optString("cc"),
                "Date" to h.optString("date"),
                "Reply-To" to h.optString("reply_to"),
            )
            for ((label, value) in rows) {
                if (value.isEmpty()) continue
                val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
                row.addView(TextView(this).apply {
                    text = label
                    setTextColor(mutedColor())
                    textSize = 13f
                    layoutParams = LinearLayout.LayoutParams(dp(64), LinearLayout.LayoutParams.WRAP_CONTENT)
                })
                row.addView(TextView(this).apply {
                    text = value
                    textSize = 13f
                    setTextIsSelectable(true)
                    layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
                })
                content.addView(row)
            }
        }

        content.addView(View(this).apply { layoutParams = LinearLayout.LayoutParams(1, dp(4)) })
        content.addView(actionRow(m))
        content.addView(View(this).apply { layoutParams = LinearLayout.LayoutParams(1, dp(4)) })

        if (!gone) {
            eventCard(m)?.let { content.addView(it) }
            inlineBanner(m)?.let { content.addView(it) }
            attachmentCard(m)?.let { content.addView(it) }
            content.addView(bodyView(m))
        }
    }

    private fun actionRow(m: JSONObject): View {
        val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        fun add(label: String, tip: String, onTap: () -> Unit) {
            row.addView(Button(this, null, android.R.attr.borderlessButtonStyle).apply {
                text = label
                contentDescription = tip
                setOnClickListener { onTap() }
            })
        }
        add("Reply", "Reply") { delegateToFlutter("reply") }
        add("Forward", "Forward") { delegateToFlutter("forward") }
        val starred = m.optBoolean("starred")
        add(if (starred) "★" else "☆", if (starred) "Unstar" else "Star") { doStar() }
        add("Delete", "Delete") { doDelete(m) }
        if (m.optBoolean("is_html") && m.optBoolean("html_colored") && dark) {
            add(if (originalColors) "Darken" else "Original", "Toggle original colours") {
                originalColors = !originalColors
                render()
            }
        }
        val more = Button(this, null, android.R.attr.borderlessButtonStyle).apply { text = "⋮" }
        more.setOnClickListener { v -> overflowMenu(v, m) }
        row.addView(more)
        return HorizontalScrollView(this).apply { addView(row) }
    }

    private fun overflowMenu(anchor: View, m: JSONObject) {
        val popup = PopupMenu(this, anchor)
        popup.menu.add("Reply all")
        popup.menu.add("Archive")
        popup.menu.add("Find similar")
        popup.menu.add("Move to…")
        popup.menu.add("Delete permanently…")
        popup.menu.add("Save as .eml…")
        popup.menu.add("Show headers…")
        if (m.optBoolean("has_remote_images") && !loadRemoteImages && !remoteOnce) {
            popup.menu.add("Show remote images")
        }
        popup.setOnMenuItemClickListener { item ->
            when (item.title.toString()) {
                "Reply all" -> delegateToFlutter("replyAll")
                "Archive" -> doArchive()
                "Find similar" -> delegateToFlutter("similar")
                "Move to…" -> doMovePicker()
                "Delete permanently…" -> doPurge(m)
                "Save as .eml…" -> doExportEml()
                "Show headers…" -> doShowHeaders()
                "Show remote images" -> {
                    remoteOnce = true
                    reload()
                }
            }
            true
        }
        popup.show()
    }

    // -- actions ----------------------------------------------------------------

    private fun delegateToFlutter(kind: String) {
        val payload = JSONObject()
            .put("kind", kind)
            .put("accountId", accountId)
            .put("folderId", folderId)
            .put("uid", uid)
            .toString()
        val intent = Intent(this, MainActivity::class.java).apply {
            action = MainActivity.ACTION_READER
            putExtra(MainActivity.EXTRA_READER_PAYLOAD, payload)
            addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        }
        startActivity(intent)
        finish()
    }

    private fun doStar() {
        bg {
            MailNative.toggleStar(accountId, folderId, uid)
            mutated = true
            val m = JSONObject(MailNative.readerMessage(folderId, uid))
            runOnUiThread {
                msg = m
                render()
                toast(if (m.optBoolean("starred")) "Starred" else "Unstarred")
            }
        }
    }

    private fun doDelete(m: JSONObject) {
        val subject = m.optString("subject")
        if (!deleteIsPermanent && !confirmDeleteSetting) {
            runDelete()
            return
        }
        confirm(
            if (deleteIsPermanent) "Delete permanently?" else "Move to Trash?",
            if (deleteIsPermanent) "“$subject” will be destroyed on the server. This cannot be undone."
            else "“$subject” will be moved to Trash.",
            if (deleteIsPermanent) "Delete permanently" else "Move to Trash",
            ::runDelete,
        )
    }

    private fun runDelete() {
        bg {
            val r = JSONObject(MailNative.deleteMessage(accountId, folderId, uid))
            runOnUiThread { afterUndoable(r) }
        }
    }

    private fun doArchive() {
        bg {
            val r = JSONObject(MailNative.archiveMessage(accountId, folderId, uid))
            runOnUiThread { afterUndoable(r) }
        }
    }

    private fun doMove(destPath: String) {
        bg {
            val r = JSONObject(MailNative.moveMessage(accountId, folderId, uid, destPath))
            runOnUiThread { afterUndoable(r) }
        }
    }

    private fun afterUndoable(r: JSONObject) {
        mutated = true
        if (r.optBoolean("purging")) {
            toast("Deleting…")
            finish()
            return
        }
        val batch = r.optString("batch")
        gone = true
        render()
        if (batch.isNotEmpty()) showUndo(r.optString("label"), batch)
        else toast(r.optString("label", "Done"))
    }

    private fun showUndo(label: String, batch: String) {
        undoBatch = batch
        undoLabel.text = label.ifEmpty { "Done" }
        undoBar.visibility = View.VISIBLE
        val grace = try {
            MailNative.undoGraceSecs().toLongOrNull() ?: 8L
        } catch (_: Exception) {
            8L
        }
        ui.postDelayed({ hideUndo() }, (grace + 1) * 1000)
    }

    private fun hideUndo() {
        undoBatch = null
        undoBar.visibility = View.GONE
    }

    private fun undoQueued() {
        val batch = undoBatch ?: return
        bg {
            val text = MailNative.undoMove(batch)
            runOnUiThread {
                hideUndo()
                gone = false
                reload()
                toast(text)
            }
        }
    }

    private fun doPurge(m: JSONObject) {
        confirm(
            "Delete permanently?",
            "“${m.optString("subject")}” will be destroyed on the server. This cannot be undone.",
            "Delete permanently",
        ) {
            bg {
                MailNative.purgeMessage(accountId, folderId, uid)
                runOnUiThread {
                    mutated = true
                    toast("Deleting…")
                    finish()
                }
            }
        }
    }

    private fun doMovePicker() {
        bg {
            val folders = JSONArray(MailNative.foldersJson(accountId))
            // `name` is the full IMAP path (see `folders_json` docs).
            val paths = (0 until folders.length())
                .map { folders.getJSONObject(it).optString("name") }
                .filter { it.isNotEmpty() }
            runOnUiThread {
                if (paths.isEmpty()) {
                    toast("No folders")
                    return@runOnUiThread
                }
                AlertDialog.Builder(this)
                    .setTitle("Move to…")
                    .setItems(paths.toTypedArray()) { _, which -> doMove(paths[which]) }
                    .setNegativeButton("Cancel", null)
                    .show()
            }
        }
    }

    private fun doShowHeaders() {
        bg {
            val h = JSONObject(MailNative.readerHeaders(folderId, uid))
            runOnUiThread {
                val rows = listOf(
                    "From" to h.optString("from"),
                    "To" to h.optString("to"),
                    "Cc" to h.optString("cc"),
                    "Date" to h.optString("date"),
                    "Subject" to h.optString("subject"),
                    "Message-ID" to h.optString("message_id"),
                    "Reply-To" to h.optString("reply_to"),
                ).filter { it.second.isNotEmpty() }
                var text = buildString {
                    for ((k, v) in rows) appendLine("$k: $v")
                    val raw = h.optString("raw")
                    if (raw.isNotEmpty()) {
                        appendLine()
                        appendLine("-- Complete headers --")
                        append(raw)
                    }
                }
                val view = TextView(this).apply {
                    text = text.ifEmpty { "No headers" }
                    setTextIsSelectable(true)
                    setPadding(dp(20), dp(8), dp(20), dp(8))
                }
                AlertDialog.Builder(this)
                    .setTitle("Headers")
                    .setView(ScrollView(this).apply { addView(view) })
                    .setPositiveButton("Close", null)
                    .show()
            }
        }
    }

    // -- event card ---------------------------------------------------------------

    private fun eventCard(m: JSONObject): View? {
        val event = m.optJSONObject("event") ?: return null
        val cancelled = event.optBoolean("is_cancelled")
        val card = cardBox(cancelled)
        card.addView(TextView(this).apply {
            text = (if (cancelled) "Cancelled: " else "") + event.optString("summary", "(Event)")
            setTypeface(typeface, Typeface.BOLD)
            maxLines = 3
        })
        card.addView(TextView(this).apply {
            text = event.optString("formatted_time")
            setTextColor(mutedColor())
        })
        val loc = event.optString("location")
        if (loc.isNotBlank()) card.addView(TextView(this).apply {
            text = "📍 $loc"
            setTextColor(mutedColor())
        })
        val org = event.optString("organizer")
        if (org.isNotBlank()) card.addView(TextView(this).apply {
            text = "Organizer: $org"
            setTextColor(mutedColor())
        })
        val attId = if (event.isNull("attachment_id")) null else event.optLong("attachment_id")
        if (attId != null) {
            val icsName = event.optString("save_name", "event.ics").ifEmpty { "event.ics" }
            val row = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.END
            }
            row.addView(Button(this@ReaderActivity, null, android.R.attr.borderlessButtonStyle).apply {
                text = "Open in Calendar"
                setOnClickListener { openAttachmentById(attId, "text/calendar") }
            })
            row.addView(Button(this@ReaderActivity, null, android.R.attr.borderlessButtonStyle).apply {
                text = "Save .ics"
                setOnClickListener { saveAttachmentById(attId, icsName, "text/calendar") }
            })
            card.addView(row)
        }
        return card
    }

    // -- inline images banner -------------------------------------------------------

    private fun inlineBanner(m: JSONObject): View? {
        val missing = m.optInt("missing_inline_images")
        if (!m.optBoolean("is_html") || missing <= 0) return null
        val box = cardBox(false)
        var busy = false
        val btn = Button(box.context, null, android.R.attr.borderlessButtonStyle)
        btn.text = "Download"
        btn.setOnClickListener {
            if (busy) return@setOnClickListener
            busy = true
            btn.text = "Downloading…"
            bg {
                MailNative.downloadMessageFiles(accountId, folderId, uid)
                runOnUiThread { reload() }
            }
        }
        box.addView(TextView(box.context).apply {
            text = "$missing inline image${if (missing == 1) "" else "s"} not downloaded"
        })
        box.addView(btn)
        return box
    }

    // -- attachments ------------------------------------------------------------------

    private fun nonInlineAttachments(m: JSONObject, hideId: Long?): List<JSONObject> {
        val arr = m.optJSONArray("attachments") ?: return emptyList()
        return (0 until arr.length())
            .map { arr.getJSONObject(it) }
            .filter { !it.optBoolean("is_inline") && it.optLong("id") != hideId }
    }

    private fun attachmentCard(m: JSONObject): View? {
        val eventId = m.optJSONObject("event")?.let {
            if (it.isNull("attachment_id")) null else it.optLong("attachment_id")
        }
        val files = nonInlineAttachments(m, eventId)
        if (files.isEmpty()) return null
        val card = cardBox(false)
        val head = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        head.addView(TextView(this).apply {
            text = if (files.size == 1) "1 attachment" else "${files.size} attachments"
            setTypeface(typeface, Typeface.BOLD)
            layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
        })
        if (files.size > 1) head.addView(Button(this, null, android.R.attr.borderlessButtonStyle).apply {
            text = "Save all"
            setOnClickListener { saveAll(files) }
        })
        card.addView(head)
        for (a in files) {
            val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
            row.addView(TextView(this).apply {
                text = "${a.optString("display_name").ifEmpty { a.optString("filename") }}  ·  ${a.optString("size_text")}"
                layoutParams = LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f)
                maxLines = 1
            })
            row.addView(Button(this, null, android.R.attr.borderlessButtonStyle).apply {
                text = "Open"
                setOnClickListener { openAttachment(a) }
            })
            row.addView(Button(this, null, android.R.attr.borderlessButtonStyle).apply {
                text = "Save"
                setOnClickListener { saveAttachment(a) }
            })
            card.addView(row)
        }
        return card
    }

    private fun ensureBytes(attachmentId: Long): ByteArray {
        try {
            return MailNative.cachedAttachmentBytes(attachmentId)
        } catch (_: Exception) {
            // Not cached: fetch from the user's own server, like Open/Save do.
        }
        runOnUiThread { toast("Downloading…") }
        MailNative.downloadMessageFiles(accountId, folderId, uid)
        return MailNative.cachedAttachmentBytes(attachmentId)
    }

    private fun attachmentDir(): String {
        val dir = java.io.File(cacheDir, "mailclient-attachments")
        dir.mkdirs()
        return dir.path
    }

    private fun openAttachment(a: JSONObject) {
        openAttachmentById(a.optLong("id"), mimeFor(a))
    }

    private fun openAttachmentById(attachmentId: Long, mime: String) {
        bg {
            val path = MailNative.writeAttachmentCopy(attachmentId, attachmentDir())
            runOnUiThread {
                val uri: Uri = FileProvider.getUriForFile(
                    this, "$packageName.readerfiles", java.io.File(path),
                )
                val view = Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, mime)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                try {
                    startActivity(Intent.createChooser(view, "Open with"))
                } catch (_: Exception) {
                    toast("No app can open this file")
                }
            }
        }
    }

    private fun saveAttachment(a: JSONObject) {
        saveAttachmentById(
            a.optLong("id"),
            a.optString("file_name").ifEmpty { a.optString("filename", "attachment.bin") },
            mimeFor(a),
        )
    }

    private fun saveAttachmentById(attachmentId: Long, name: String, mime: String) {
        bg {
            val bytes = ensureBytes(attachmentId)
            runOnUiThread {
                pendingSaveBytes = bytes
                pendingSaveName = name
                pendingSaveMime = mime
                val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = mime
                    putExtra(Intent.EXTRA_TITLE, name)
                }
                @Suppress("DEPRECATION")
                startActivityForResult(intent, REQ_SAVE_ONE)
            }
        }
    }

    private fun saveAll(files: List<JSONObject>) {
        bg {
            val pending = files.map {
                val bytes = ensureBytes(it.optLong("id"))
                val name = it.optString("file_name").ifEmpty { it.optString("filename", "attachment.bin") }
                PendingFile(name, mimeFor(it), bytes)
            }
            runOnUiThread {
                pendingSaveAll = pending
                val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                @Suppress("DEPRECATION")
                startActivityForResult(intent, REQ_SAVE_ALL)
            }
        }
    }

    private fun doExportEml() {
        bg {
            runOnUiThread { toast("Preparing export…") }
            val bytes = MailNative.exportEmlBytes(folderId, uid)
            val name = MailNative.suggestedEmlName(folderId, uid)
            runOnUiThread {
                pendingEmlBytes = bytes
                pendingEmlName = name
                val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = "message/rfc822"
                    putExtra(Intent.EXTRA_TITLE, name)
                }
                @Suppress("DEPRECATION")
                startActivityForResult(intent, REQ_SAVE_EML)
            }
        }
    }

    @Deprecated("Without AndroidX activity-result; still dispatched.")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        when (requestCode) {
            REQ_SAVE_ONE -> {
                val bytes = pendingSaveBytes ?: return
                bg { writeUri(uri, bytes); runOnUiThread { toast("Saved") } }
            }
            REQ_SAVE_EML -> {
                val bytes = pendingEmlBytes ?: return
                bg { writeUri(uri, bytes); runOnUiThread { toast("Exported to $pendingEmlName") } }
            }
            REQ_SAVE_ALL -> {
                val files = pendingSaveAll
                bg {
                    var n = 0
                    for (f in files) {
                        val doc = DocumentsContract.createDocument(
                            contentResolver, uri, f.mime, f.name,
                        ) ?: continue
                        try {
                            contentResolver.openOutputStream(doc)?.use { it.write(f.bytes) }
                            n++
                        } catch (_: Exception) {
                        }
                    }
                    val done = n
                    runOnUiThread { toast("Saved $done file(s)") }
                }
            }
        }
    }

    private fun writeUri(uri: Uri, bytes: ByteArray) {
        contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
            ?: throw IllegalStateException("cannot write")
    }

    // -- body --------------------------------------------------------------------------

    private fun bodyView(m: JSONObject): View {
        if (!m.optBoolean("is_html")) {
            return TextView(this).apply {
                text = m.optString("body_text")
                textSize = 16f * readerScale * resources.configuration.fontScale
                setTextIsSelectable(true)
                setPadding(0, dp(8), 0, 0)
            }
        }
        val web = WebView(this).apply {
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT,
            )
            // Fully expanded to its content so there is nothing to scroll
            // internally: the parent ScrollView owns the one fling. (If a
            // device still routes drags into the page, the follow-up is a
            // NestedScrollWebView port.)
            isVerticalScrollBarEnabled = false
            isHorizontalScrollBarEnabled = false
            overScrollMode = View.OVER_SCROLL_NEVER
            settings.apply {
                javaScriptEnabled = false
                allowFileAccess = false
                allowContentAccess = false
                mediaPlaybackRequiresUserGesture = true
                setGeolocationEnabled(false)
                @Suppress("DEPRECATION")
                textZoom = (100 * readerScale * resources.configuration.fontScale).toInt()
            }
            val t = themeColors()
            setBackgroundColor(0xFF000000.toInt() or t[0])
            webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
                    val url = request.url.toString()
                    if (url.startsWith("about:")) return false
                    onTapUrl(url)
                    return true
                }
            }
        }
        web.loadDataWithBaseURL(null, buildDoc(m.optString("body_html")), "text/html", "utf-8", null)
        return web
    }

    private fun onTapUrl(url: String) {
        bg {
            val info = JSONObject(MailNative.linkInfo(url))
            runOnUiThread {
                if (!info.optBoolean("safe")) {
                    toast("Link blocked")
                    return@runOnUiThread
                }
                if (linkClickAction == "browser") {
                    openBrowser(url)
                    return@runOnUiThread
                }
                val body = TextView(this).apply {
                    text = "Address\n$url\n\nScheme\n${part(info, "scheme")}\n\nDomain\n${part(info, "host")}\n\nPath\n${part(info, "path")}"
                    setTextIsSelectable(true)
                    setPadding(dp(20), dp(8), dp(20), dp(8))
                }
                AlertDialog.Builder(this)
                    .setTitle("Examine link")
                    .setView(ScrollView(this).apply { addView(body) })
                    .setNeutralButton("Copy") { _, _ ->
                        (getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager)?.setPrimaryClip(
                            android.content.ClipData.newPlainText("link", url),
                        )
                        toast("Link copied")
                    }
                    .setNegativeButton("Close", null)
                    .setPositiveButton("Open in browser") { _, _ -> openBrowser(url) }
                    .show()
            }
        }
    }

    private fun part(info: JSONObject, key: String): String {
        val v = info.optString(key)
        return v.ifEmpty { "—" }
    }

    private fun openBrowser(url: String) {
        try {
            startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url.trim())))
        } catch (_: Exception) {
            toast("Cannot open link")
        }
    }

    // -- helpers --------------------------------------------------------------------------

    private fun confirm(title: String, text: String, ok: String, run: () -> Unit) {
        AlertDialog.Builder(this)
            .setTitle(title)
            .setMessage(text)
            .setNegativeButton("Cancel", null)
            .setPositiveButton(ok) { _, _ -> run() }
            .show()
    }

    private fun cardBox(alert: Boolean): LinearLayout {
        val border = if (alert) 0xFFB3261E.toInt() else mutedColor()
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(12), dp(10), dp(12), dp(10))
            background = GradientDrawable().apply {
                shape = GradientDrawable.RECTANGLE
                cornerRadius = dp(8).toFloat()
                setStroke(dp(1), border)
            }
            layoutParams = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT,
            ).apply { setMargins(0, dp(6), 0, dp(2)) }
        }
    }

    private fun dp(v: Int): Int = (v * resources.displayMetrics.density).toInt()

    private fun mutedColor(): Int =
        attrColor(android.R.attr.textColorSecondary, if (dark) 0xFFCAC4D0.toInt() else 0xFF5F6368.toInt())

    private fun attrColor(attr: Int, fallback: Int): Int {
        val out = TypedValue()
        return if (theme.resolveAttribute(attr, out, true)) {
            when (out.type) {
                TypedValue.TYPE_INT_COLOR_ARGB8, TypedValue.TYPE_INT_COLOR_RGB8 -> out.data
                else -> fallback
            }
        } else {
            fallback
        }
    }

    private fun parseCss(css: String, fallback: Int): Int {
        val c = css.trim()
        if (c.length == 7 && c[0] == '#' && c.drop(1).all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }) {
            return try {
                0xFF000000.toInt() or c.substring(1).toInt(16)
            } catch (_: NumberFormatException) {
                fallback
            }
        }
        return fallback
    }

    // -- attachment MIME --------------------------------------------------------------------
    //
    // Port of the Dart `openMimeType`: the stored type canonicalized, or the
    // filename's extension when the stored type is missing or generic. Never
    // empty: the opener falls back to `*/*` rather than a lie.

    private fun mimeFor(a: JSONObject): String {
        val stored = a.optString("mime_type").trim().lowercase()
        if (stored !in setOf("", "application/octet-stream", "application/unknown", "application/binary")) {
            return canonMime(stored)
        }
        val name = a.optString("file_name").ifEmpty { a.optString("filename") }
        val ext = name.substringAfterLast('.', "").lowercase()
        return mimeByExt[ext] ?: "*/*"
    }

    private fun canonMime(m: String): String = when (m) {
        "image/jpg", "image/x-png" -> "image/jpeg"
        "application/ics", "text/x-vcalendar", "application/x-vcalendar", "text/x-vcal" -> "text/calendar"
        "text/x-vcard", "text/directory" -> "text/vcard"
        "audio/x-mp3" -> "audio/mpeg"
        else -> m
    }

    private val mimeByExt = mapOf(
        "ics" to "text/calendar", "vcf" to "text/vcard", "eml" to "message/rfc822",
        "pdf" to "application/pdf", "html" to "text/html", "htm" to "text/html",
        "txt" to "text/plain", "csv" to "text/csv", "json" to "application/json",
        "xml" to "application/xml", "zip" to "application/zip", "png" to "image/png",
        "jpg" to "image/jpeg", "jpeg" to "image/jpeg", "gif" to "image/gif",
        "webp" to "image/webp", "svg" to "image/svg+xml", "mp3" to "audio/mpeg",
        "wav" to "audio/wav", "ogg" to "audio/ogg", "flac" to "audio/flac",
        "mp4" to "video/mp4", "doc" to "application/msword",
        "docx" to "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" to "application/vnd.ms-excel",
        "xlsx" to "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" to "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" to "application/vnd.oasis.opendocument.text",
    )
}
