package de.renier.mailclient

import android.content.Context
import android.os.Build
import android.util.Log
import androidx.work.Constraints
import androidx.work.Data
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequest
import androidx.work.OutOfQuotaPolicy
import androidx.work.PeriodicWorkRequest
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit

// One background mail check, run by WorkManager for the battery-saving
// periodic schedule and for every exact-alarm shot (MailAlarm.kt). Calls the
// Rust core directly, so no Flutter engine starts for it. A due scheduled
// send runs here too (trigger "scheduled-send"), but as an outbox-only
// flush — never the inbox check below.
class MailCheckWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    override fun doWork(): Result {
        try {
            MailNative.ensureInit(applicationContext)
            val trigger = inputData.getString(KEY_TRIGGER) ?: "worker"
            if (trigger == "scheduled-send") {
                // Outbox-only: no inbox sync, no notification. Quiet hours
                // gate new-mail checks; they never hold back a send the
                // user scheduled.
                MailNative.sendDue()
                MailSendAlarm.refresh(applicationContext)
                return Result.success()
            }
            // The check the schedulers ask for: what they owe now, so manual
            // accounts are left to their own syncs (the Quick Settings tile
            // runs its own check, marked explicit, in the shade instead).
            val report = MailNative.check(trigger, now = false)
            MailNotifier.deliver(applicationContext, report)
            // A due scheduled send may just have gone out (or failed): the next
            // held row needs its alarm.
            MailSendAlarm.refresh(applicationContext)
            // The tap answers even when it found nothing, so the user is not
            // left guessing whether the tile did anything at all.
            if (trigger == "tile") CheckFeedback.show(applicationContext, report)
            return Result.success()
        } catch (e: RuntimeException) {
            Log.w("mailclient", "background check failed", e)
            // The tap still answers, even when the check itself blew up.
            if ((inputData.getString(KEY_TRIGGER) ?: "worker") == "tile") {
                CheckFeedback.showFailure(applicationContext)
            }
            // The shot is spent either way: re-arm from what is still held.
            runCatching { MailSendAlarm.refresh(applicationContext) }
            return Result.failure()
        }
    }

    companion object {
        const val KEY_TRIGGER = "trigger"

        // WorkManager name of the periodic check. The same name the Dart
        // workmanager plugin used, so scheduling replaces its old entry.
        private const val PERIODIC = "mail-background-sync"

        // WorkManager name of a pending scheduled send. Separate from the
        // poller's check task in MailAlarm: sharing one name with KEEP
        // would drop the send while a check waits for the network (or the
        // check while a send waits).
        private const val SEND_TASK = "mail-scheduled-send"

        // Android's floor for periodic work.
        private const val MIN_MINUTES = 15L

        private const val PREFS = "mailclient_worker"
        private const val KEY_NATIVE = "native_periodic"

        fun input(trigger: String): Data = Data.Builder().putString(KEY_TRIGGER, trigger).build()

        // A due scheduled send: outbox-only flush, kept off the poller's
        // task so neither drops the other. APPEND chains shots (each flush
        // is quick once the first sent); KEEP would drop a second send
        // while the first still waits for the network.
        fun enqueueSendDue(context: Context) {
            val request = OneTimeWorkRequest.Builder(MailCheckWorker::class.java)
                .setInputData(input("scheduled-send"))
                .setConstraints(
                    Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build(),
                )
                .apply {
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                        setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
                    }
                }
                .build()
            WorkManager.getInstance(context)
                .enqueueUniqueWork(SEND_TASK, ExistingWorkPolicy.APPEND, request)
        }

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
