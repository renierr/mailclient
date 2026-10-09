package de.renier.mailclient

import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray

/**
 * The "Check mail" tile of the Quick Settings shade: a one-tap mail check
 * for people who set background checking off, and for anyone who would
 * rather pull the shade down than open the app and press Sync.
 *
 * The tap runs the very check the background schedulers run — WorkManager
 * expedited work, [MailCheckWorker], `mailcore::sync::background` in the
 * Rust core — only marked explicit, so it also serves the accounts the
 * scheduler skips: manual ones, and any inside their quiet hours. Quiet
 * hours gate unattended checks; a tap is the user asking.
 *
 * The user adds the tile once through the shade's edit mode, and from then
 * on it lives there. Two rules the tile keeps no matter what the phone is
 * doing:
 *
 * - **Every state answers a tap.** `STATE_UNAVAILABLE` is reserved for
 *   "nothing to check yet" (its tap opens the app to fix that). While a
 *   check runs, a tap answers "Still checking mail…" instead of being
 *   dropped: on a real phone the work can sit queued a while (network,
 *   battery optimisation, Doze), and a dead tile is indistinguishable
 *   from a broken one.
 * - **The answer stays on the tile.** A click does not collapse the shade
 *   on Android 12+, so the check's answer — "No new mail", "3 new
 *   messages", "Mail check failed" — shows as the tile's subtitle while
 *   the shade is up, and as a toast when one can be posted.
 */
class MailCheckTileService : TileService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var stopObservingChecks: (() -> Unit)? = null

    override fun onStartListening() {
        super.onStartListening()
        render()
        // Without this the tile would sit in its busy state until the
        // shade is next opened: a click keeps the shade up.
        stopObservingChecks = MailAlarm.observeChecks(applicationContext) { render() }
    }

    override fun onStopListening() {
        stopObservingChecks?.invoke()
        stopObservingChecks = null
        super.onStopListening()
    }

    override fun onClick() {
        super.onClick()
        scope.launch {
            val accounts = withContext(Dispatchers.IO) { accountCount() }
            if (accounts == 0) {
                // Nothing to check yet: open the app rather than sit dead.
                openApp()
                return@launch
            }
            when (withContext(Dispatchers.IO) { MailAlarm.checkState(applicationContext) }) {
                CheckState.Running -> answer("Still checking mail…")
                CheckState.Queued -> answer("Still waiting to check…")
                CheckState.Idle -> {
                    // The last answer has been seen; the next one is owed.
                    CheckFeedback.takeOutcome()
                    // KEEP inside: a check already waiting is reused.
                    MailAlarm.enqueueCheck(applicationContext, "tile", now = true)
                    show(busy = true, accounts = accounts)
                }
            }
        }
    }

    override fun onDestroy() {
        stopObservingChecks?.invoke()
        stopObservingChecks = null
        scope.cancel()
        super.onDestroy()
    }

    // Reading the account list touches SQLite and asking WorkManager for
    // its state blocks; neither belongs on the main thread.
    private fun render() {
        scope.launch {
            val accounts = withContext(Dispatchers.IO) { accountCount() }
            val state = withContext(Dispatchers.IO) { MailAlarm.checkState(applicationContext) }
            val outcome = CheckFeedback.takeOutcome()
            show(busy = state != CheckState.Idle, accounts = accounts, outcome = outcome)
        }
    }

    private fun answer(line: String) = CheckFeedback.toast(this, line)

    private fun show(busy: Boolean, accounts: Int, outcome: String? = null) {
        val tile = qsTile ?: return
        val canSubtitle = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
        val label = getString(R.string.tile_check_mail)
        when {
            accounts == 0 -> {
                tile.state = Tile.STATE_UNAVAILABLE
                tile.label = label
                if (canSubtitle) tile.subtitle = "Add an account in the app"
            }

            busy -> {
                // Greyed would block the tap; a live tile can always answer.
                tile.state = Tile.STATE_INACTIVE
                tile.label = label
                if (canSubtitle) tile.subtitle = "Checking mail…"
            }

            outcome != null -> {
                tile.state = Tile.STATE_INACTIVE
                tile.label = label
                if (canSubtitle) tile.subtitle = outcome
            }

            else -> {
                // Idle: label only, like the other quiet tiles of the shade.
                tile.state = Tile.STATE_INACTIVE
                tile.label = label
                if (canSubtitle) tile.subtitle = null
            }
        }
        runCatching { tile.updateTile() }
            .onFailure { Log.w("mailclient", "tile update failed", it) }
    }

    /** The app itself, for the tile that has nothing to check yet. */
    private fun openApp() {
        val intent = Intent(this, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                startActivityAndCollapse(
                    PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE)
                )
            } else {
                @Suppress("DEPRECATION")
                startActivityAndCollapse(intent)
            }
        }.onFailure { startActivity(intent) }
    }

    /** Accounts the core knows; `-1` when the database could not be read. */
    private fun accountCount(): Int = runCatching {
        MailNative.ensureInit(applicationContext)
        JSONArray(MailNative.accountsJson()).length()
    }.getOrDefault(-1)
}
