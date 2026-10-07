package de.renier.mailclient.ui.state

import de.renier.mailclient.ui.composer.ComposerSeed

// Step 1 shell state: what the app shows and what it does, over the 0a–0e
// JNI surface. Plain holder (no new dependencies — no ViewModel, no
// navigation-compose), owned by MailShell's composition. Reads take explicit
// ids and jobs only say *that* something changed, so this re-reads whatever
// is showing — the same contract the Dart MailState keeps.
data class Account(
    val id: Long,
    val email: String,
    val name: String,
    // The display name sent in From (the composer prefills it).
    val fromName: String = "",
    val initials: String = "?",
    val avatarLight: String = "",
    val avatarDark: String = "",
)

data class Folder(
    val id: Long,
    val path: String,
    val leaf: String,
    val depth: Int,
    val role: String,
    val unread: Int,
    val count: Int,
    // Sidebar visibility only: hidden folders keep their cache and syncing.
    val subscribed: Boolean = true,
    // Collapse rule from the feed (`FolderRole::always_visible`): known
    // folders stay visible inside a collapsed parent; only custom
    // subfolders fold away.
    val alwaysVisible: Boolean = true,
    // Delete here destroys instead of moving to Trash (core decides).
    val deleteIsPermanent: Boolean = false,
)

// The settings the reader acts on, from the core's settingsJson.
data class ReaderPrefs(
    val autoMarkRead: Boolean = true,
    val markReadDelaySecs: Long = 0,
    val loadRemoteImages: Boolean = false,
    val confirmDelete: Boolean = true,
    val linkClickAction: String = "examine",
    // Text size multiplier: the Qt reader's 12 / 14 / 18 px steps.
    val scale: Float = 1f,
)

data class MessageRow(
    val uid: Int,
    val subject: String,
    val from: String,
    val fromName: String,
    val date: String,
    val snippet: String,
    val unread: Boolean,
    val starred: Boolean,
    val hasAttachments: Boolean,
    // Raw UTC timestamp for the date quick-filter (`dateFilterMatches`);
    // `date` above is display text.
    val dateRaw: String = "",
    // Core-decided avatar (mailcore::badge): initials + per-theme hex.
    val initials: String = "?",
    val avatarLight: String = "",
    val avatarDark: String = "",
    // Search hits span folders and carry their own; list rows leave -1.
    val folderId: Long = -1,
)

data class UndoOffer(val batch: String, val label: String)

/** What a delete does and whether to ask first (`mailcore::undo::delete_prompt`). */
data class DeletePrompt(val permanent: Boolean, val ask: Boolean)

/** A composition sent but not yet accepted by SMTP, as the composer held it. */
data class PendingSend(val seed: ComposerSeed, val accountId: Long, val folderId: Long)

/**
 * One painted sidebar row: the folder's id plus its collapse state and the
 * counts to show, folded by the core (`mailcore::feed::sidebar_rows`). A
 * collapsed parent already aggregates its hidden children's counts.
 * Path, role, depth and leaf still come from [Folder], joined by id.
 */
data class SidebarRow(
    val id: Long,
    val collapsible: Boolean,
    val expanded: Boolean,
    val unread: Int,
    val total: Int,
)

/** A queue call refused because the same job is in flight: not an error. */
internal fun Throwable.isAlreadyRunning(): Boolean =
    message.orEmpty().contains("already running", ignoreCase = true)
