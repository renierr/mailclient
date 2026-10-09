package de.renier.mailclient

import android.content.Context
import android.util.Log
import androidx.work.Constraints
import androidx.work.Data
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequest
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

// One background mail check, run by WorkManager for the battery-saving
// periodic schedule and for every exact-alarm shot (MailAlarm.kt). Calls the
// Rust core directly, so no Flutter engine starts for it.
class MailCheckWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    override fun doWork(): Result = try {
        MailNative.ensureInit(applicationContext)
        val trigger = inputData.getString(KEY_TRIGGER) ?: "worker"
        // `now` marks an explicit check (the "Check mail" Quick Settings
        // tile): every account, quiet hours included.
        val report = MailNative.check(trigger, inputData.getBoolean(KEY_NOW, false))
        MailNotifier.deliver(applicationContext, report)
        // The tap answers even when it found nothing, so the user is not
        // left guessing whether the tile did anything at all.
        if (trigger == "tile") CheckFeedback.show(applicationContext, report)
        Result.success()
    } catch (e: RuntimeException) {
        Log.w("mailclient", "background check failed", e)
        // The tap still answers, even when the check itself blew up.
        if ((inputData.getString(KEY_TRIGGER) ?: "worker") == "tile") {
            CheckFeedback.showFailure(applicationContext)
        }
        Result.failure()
    }

    companion object {
        const val KEY_TRIGGER = "trigger"
        private const val KEY_NOW = "now"

        // WorkManager name of the periodic check. The same name the Dart
        // workmanager plugin used, so scheduling replaces its old entry.
        // Read by MailAlarm.checkRunning, so the "Check mail" tile can tell
        // that a scheduled check is in flight.
        internal const val PERIODIC = "mail-background-sync"

        // Android's floor for periodic work.
        private const val MIN_MINUTES = 15L

        private const val PREFS = "mailclient_worker"
        private const val KEY_NATIVE = "native_periodic"

        fun input(trigger: String, now: Boolean = false): Data =
            Data.Builder().putString(KEY_TRIGGER, trigger).putBoolean(KEY_NOW, now).build()

        // The battery-saving schedule: deferrable, only with a network.
        fun schedulePeriodic(context: Context, minutes: Int) {
            if (minutes <= 0) {
                cancelPeriodic(context)
                return
            }
            val every = maxOf(minutes.toLong(), MIN_MINUTES)
            val request = PeriodicWorkRequest.Builder(MailCheckWorker::class.java, every, TimeUnit.MINUTES)
                .setInputData(input("worker"))
                .setInitialDelay(every, TimeUnit.MINUTES)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            // UPDATE keeps the running schedule, so opening the app does not
            // push the next check back. Once, the entry the Dart plugin left
            // (another worker class) is replaced instead.
            val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val policy = if (prefs.getBoolean(KEY_NATIVE, false)) {
                ExistingPeriodicWorkPolicy.UPDATE
            } else {
                ExistingPeriodicWorkPolicy.CANCEL_AND_REENQUEUE
            }
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(PERIODIC, policy, request)
            prefs.edit().putBoolean(KEY_NATIVE, true).apply()
        }

        fun cancelPeriodic(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(PERIODIC)
        }
    }
}
