package de.renier.mailclient

import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
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
 * It runs the very check the background schedulers run —
 * `mailcore::sync::background` over JNI — only marked explicit, so it also
 * serves the accounts the scheduler skips: manual ones, and any inside
 * their quiet hours. Quiet hours gate unattended checks; a tap is the user
 * asking.
 *
 * Why the tap does the work here instead of handing it to WorkManager like
 * the schedulers do: a tile lives while the shade is open, and the tap is
 * the user waiting behind it. A phone may defer background work for a long
 * time (battery restriction, no network when the job was posted), and a
 * tile that says "checking" for minutes — or nothing at all — looks broken.
 * Running it here answers in seconds whether or not the OS would run a job.
 * The schedulers keep their worker path; nothing else changes.
 *
 * Every tap is acknowledged ("Checking mail…"), and the answer lands as a
 * toast and on the tile itself ("No new mail", "3 new messages", "Mail
 * check failed") — a click does not collapse the shade on Android 12+, and
 * a toast can be suppressed on a phone, so the tile shows it too.
 */
class MailCheckTileService : TileService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    @Volatile
    private var checking = false

    override fun onStartListening() {
        super.onStartListening()
        val accounts = accountCount()
        show(busy = checking, accounts = accounts)
    }

    override fun onClick() {
        super.onClick()
        if (checking) {
            // The check is already under way; the tile never blocks a tap,
            // it answers it.
            CheckFeedback.toast(this, "Still checking mail…")
            return
        }
        scope.launch {
            val accounts = withContext(Dispatchers.IO) { accountCount() }
            if (accounts == 0) {
                // Nothing to check yet: open the app rather than sit dead.
                openApp()
                return@launch
            }
            checking = true
            checkNow(accounts)
        }
    }

    override fun onDestroy() {
        // The check itself cannot be interrupted mid-call, and its answer is
        // delivered from a non-cancellable block below, so closing the shade
        // early costs a moment of silence, never a half-answered tap.
        scope.cancel()
        super.onDestroy()
    }

    private fun checkNow(accounts: Int) {
        scope.launch {
            CheckFeedback.toast(this@MailCheckTileService, "Checking mail…")
            show(busy = true, accounts = accounts)
            val report = withContext(Dispatchers.IO) {
                runCatching {
                    MailNative.ensureInit(applicationContext)
                    MailNative.check("tile", now = true)
                }.getOrNull()
            }
            withContext(NonCancellable) {
                checking = false
                val line = if (report == null) {
                    CheckFeedback.showFailure(this@MailCheckTileService)
                    "Mail check failed"
                } else {
                    withContext(Dispatchers.IO) {
                        MailNotifier.deliver(applicationContext, report)
                    }
                    CheckFeedback.show(this@MailCheckTileService, report)
                    CheckFeedback.takeOutcome() ?: "Mail check failed"
                }
                show(busy = false, accounts = accounts, outcome = line)
            }
        }
    }

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
