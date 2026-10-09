package de.renier.mailclient

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
 * on it lives there. A click does not collapse the shade on Android 12+,
 * so the tile itself carries the whole story: greyed out with "Checking
 * mail…" while the check runs, and [CheckFeedback] answers with a toast
 * when it is done ("No new mail", or how many messages arrived).
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
        // KEEP inside: a check already waiting for the network is reused,
        // so what the tile is about to show is honest.
        MailAlarm.enqueueCheck(applicationContext, "tile", now = true)
        scope.launch {
            show(active = true, accounts = withContext(Dispatchers.IO) { accountCount() })
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
            val active = withContext(Dispatchers.IO) { MailAlarm.checkRunning(applicationContext) }
            show(active = active, accounts = accounts)
        }
    }

    private fun show(active: Boolean, accounts: Int) {
        val tile = qsTile ?: return
        val canSubtitle = Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
        val label = getString(R.string.tile_check_mail)
        when {
            accounts == 0 -> {
                // Nothing to check yet, so the tile says what to do about it.
                tile.state = Tile.STATE_UNAVAILABLE
                tile.label = label
                if (canSubtitle) tile.subtitle = "Add an account in the app"
            }

            active -> {
                // Busy: greyed out, so a second tap cannot stack a check.
                tile.state = Tile.STATE_UNAVAILABLE
                tile.label = label
                if (canSubtitle) tile.subtitle = "Checking mail…"
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

    /** Accounts the core knows; `-1` when the database could not be read. */
    private fun accountCount(): Int = runCatching {
        MailNative.ensureInit(applicationContext)
        JSONArray(MailNative.accountsJson()).length()
    }.getOrDefault(-1)
}
