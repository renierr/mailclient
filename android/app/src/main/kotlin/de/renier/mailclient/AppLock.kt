package de.renier.mailclient

import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import android.content.Intent
import android.hardware.biometrics.BiometricManager
import android.hardware.biometrics.BiometricPrompt
import android.os.Build
import android.os.CancellationSignal
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.Lifecycle
import org.json.JSONObject
import kotlin.concurrent.thread

// Optional app lock (Settings → Privacy, `app_lock_enabled`): on a cold
// start, and on return after `app_lock_after_secs` away, a cover hides the
// shell until the fingerprint (or the phone's screen lock) confirms. The
// shell stays composed underneath, so an open draft survives a lock.
// Framework BiometricPrompt (Android 9+, no dependency); the screen lock is
// always accepted too, so a failing sensor never locks anyone out.
object AppLock {
    enum class Support { Ready, NoScreenLock, Unsupported }

    /** Null until the settings are read (cover without a prompt), then whether the cover shows. */
    var locked by mutableStateOf<Boolean?>(null)
        private set

    /** The last prompt's failure, for the cover; null when there is none. */
    var error by mutableStateOf<String?>(null)
        private set

    private var enabled = false
    private var afterMs = 60_000L
    private var loaded = false
    private var leftAt: Long? = null

    // A prompt (or the screen-lock page it opens) is up: the activity
    // stopping under it is not the user leaving.
    private var authenticating = false
    private var promptedThisLock = false

    private var activity: MainActivity? = null
    private var screenLockResult: ((Boolean) -> Unit)? = null
    private val main = Handler(Looper.getMainLooper())

    fun support(context: Context): Support {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) return Support.Unsupported
        val keyguard = context.getSystemService(KeyguardManager::class.java)
        return if (keyguard?.isDeviceSecure == true) Support.Ready else Support.NoScreenLock
    }

    // --- MainActivity lifecycle --------------------------------------------

    fun attach(activity: MainActivity) {
        this.activity = activity
        reload()
    }

    fun detach(activity: MainActivity) {
        if (this.activity === activity) this.activity = null
    }

    fun onStart(activity: MainActivity) {
        if (!loaded || authenticating) return
        val away = leftAt?.let { SystemClock.elapsedRealtime() - it }
        leftAt = null
        if (enabled && away != null && away >= afterMs) lock()
    }

    fun onResume(activity: MainActivity) {
        if (locked == true && !authenticating && !promptedThisLock) {
            promptedThisLock = true
            unlock(activity)
        }
    }

    fun onStop(activity: MainActivity) {
        if (activity.isChangingConfigurations || authenticating) return
        leftAt = SystemClock.elapsedRealtime()
        promptedThisLock = false
    }

    /** The screen-lock page's answer (API 28, or no fingerprint enrolled). */
    fun onScreenLockResult(ok: Boolean) {
        val done = screenLockResult
        screenLockResult = null
        done?.invoke(ok)
    }

    /**
     * Re-read the two settings (after Save, and on attach). The first read
     * of a process locks when the lock is on: that is the cold start.
     */
    fun reload() {
        thread(name = "app-lock-settings", isDaemon = true) {
            val read = runCatching {
                val a = activity ?: return@thread
                MailNative.ensureInit(a)
                val s = JSONObject(MailNative.settingsJson())
                s.optBoolean("app_lock_enabled", false) to s.optLong("app_lock_after_secs", 60L)
            }.onFailure { Log.w("mailclient", "app lock: cannot read settings", it) }
            main.post { apply(read.getOrNull()) }
        }
    }

    private fun apply(settings: Pair<Boolean, Long>?) {
        val first = !loaded
        loaded = true
        // An unreadable setting keeps a lock that is already on.
        val (on, afterSecs) = settings ?: (enabled to afterMs / 1000)
        enabled = on && activity?.let { support(it) } == Support.Ready
        afterMs = afterSecs * 1000
        activity?.let { a ->
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) a.setRecentsScreenshotEnabled(!enabled)
        }
        when {
            !enabled -> {
                locked = false
                error = null
            }
            first -> lock()
            locked == null -> locked = false
        }
        val a = activity
        if (a != null && a.lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) onResume(a)
    }

    private fun lock() {
        locked = true
        error = null
        promptedThisLock = false
    }

    // --- Prompting ----------------------------------------------------------

    /** Prompt, and lift the cover on success. */
    fun unlock(activity: Activity) {
        if (support(activity) != Support.Ready) {
            // The phone lost its screen lock: there is nothing left to check against.
            locked = false
            error = null
            return
        }
        authenticate(activity, "Unlock Mailclient") { ok, message ->
            if (ok) {
                locked = false
                error = null
            } else {
                error = message
            }
        }
    }

    /**
     * Confirm the user with the fingerprint, or the screen lock. [done] gets
     * success, or a reason to show (null when the user just cancelled).
     */
    fun authenticate(activity: Activity, title: String, done: (Boolean, String?) -> Unit) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
            done(false, "Needs Android 9 or newer")
            return
        }
        authenticating = true
        val finish = { ok: Boolean, message: String? ->
            authenticating = false
            done(ok, message)
        }
        val builder = BiometricPrompt.Builder(activity)
            .setTitle(title)
            .setSubtitle("Fingerprint or screen lock")
        when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.R ->
                builder.setAllowedAuthenticators(
                    BiometricManager.Authenticators.BIOMETRIC_WEAK or BiometricManager.Authenticators.DEVICE_CREDENTIAL,
                )
            Build.VERSION.SDK_INT == Build.VERSION_CODES.Q ->
                @Suppress("DEPRECATION")
                builder.setDeviceCredentialAllowed(true)
            else ->
                builder.setNegativeButton("Use screen lock", activity.mainExecutor) { _, _ ->
                    confirmScreenLock(activity, title, finish)
                }
        }
        val callback = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                finish(true, null)
            }

            override fun onAuthenticationError(code: Int, message: CharSequence) {
                when (code) {
                    BiometricPrompt.BIOMETRIC_ERROR_USER_CANCELED,
                    BiometricPrompt.BIOMETRIC_ERROR_CANCELED,
                    -> finish(false, null)
                    // No usable sensor, or too many tries: the screen lock
                    // answers instead (Android 9's prompt has no such option).
                    BiometricPrompt.BIOMETRIC_ERROR_NO_BIOMETRICS,
                    BiometricPrompt.BIOMETRIC_ERROR_LOCKOUT,
                    BiometricPrompt.BIOMETRIC_ERROR_LOCKOUT_PERMANENT,
                    BiometricPrompt.BIOMETRIC_ERROR_HW_NOT_PRESENT,
                    BiometricPrompt.BIOMETRIC_ERROR_HW_UNAVAILABLE,
                    -> confirmScreenLock(activity, title, finish)
                    else -> finish(false, message.toString())
                }
            }
        }
        runCatching { builder.build().authenticate(CancellationSignal(), activity.mainExecutor, callback) }
            .onFailure { finish(false, it.message ?: "Could not ask for the fingerprint") }
    }

    private fun confirmScreenLock(activity: Activity, title: String, finish: (Boolean, String?) -> Unit) {
        val keyguard = activity.getSystemService(KeyguardManager::class.java)
        @Suppress("DEPRECATION")
        val intent: Intent? = keyguard?.createConfirmDeviceCredentialIntent(title, null)
        val host = this.activity
        if (intent == null || host == null) {
            finish(false, "No screen lock to confirm with")
            return
        }
        screenLockResult = { ok -> finish(ok, null) }
        host.confirmScreenLock(intent)
    }
}
