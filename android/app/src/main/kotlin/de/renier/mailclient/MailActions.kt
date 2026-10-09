package de.renier.mailclient

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.util.Log
import androidx.work.Constraints
import androidx.work.Data
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequest
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters

// The notifications' "Mark read" / "Mark all read" buttons. Marks the cache
// at once and re-plans the notifications (the read mail drops out of them),
// then leaves the server to MailFlagWorker, which waits for a network.
class MailActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != ACTION_MARK_READ) return
        val target = intent.getStringExtra(EXTRA_TARGET) ?: return
        val app = context.applicationContext
        val pending = goAsync()
        Thread {
            try {
                MailNative.ensureInit(app)
                MailNotifier.deliver(app, MailNative.markRead(target))
                MailNotifier.onMailChanged?.invoke()
                MailFlagWorker.enqueue(app, MailNative.readTargetAccount(target))
            } catch (e: RuntimeException) {
                Log.w("mailclient", "mark read from notification failed", e)
            } finally {
                pending.finish()
            }
        }.start()
    }

    companion object {
        private const val ACTION_MARK_READ = "de.renier.mailclient.MARK_READ"
        private const val EXTRA_TARGET = "target"

        // One PendingIntent per notification (the tag goes into the data
        // URI), each carrying the ReadTarget JSON its plan gave it.
        fun markReadIntent(context: Context, tag: String, target: String): PendingIntent {
            val intent = Intent(context, MailActionReceiver::class.java)
                .setAction(ACTION_MARK_READ)
                .setData(Uri.fromParts("mailclient", tag, null))
                .putExtra(EXTRA_TARGET, target)
            return PendingIntent.getBroadcast(
                context,
                0,
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }
    }
}

// Carries read flags set from a notification to the server, over a fresh
// connection. WorkManager holds it until a network is up and retries a
// failed push; a sync pushes anything still queued anyway.
class MailFlagWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    override fun doWork(): Result = try {
        MailNative.ensureInit(applicationContext)
        MailNative.pushFlags(inputData.getLong(KEY_ACCOUNT, 0))
        Result.success()
    } catch (e: RuntimeException) {
        Log.w("mailclient", "flag push failed", e)
        if (runAttemptCount < MAX_ATTEMPTS) Result.retry() else Result.failure()
    }

    companion object {
        private const val KEY_ACCOUNT = "account_id"
        private const val MAX_ATTEMPTS = 3

        fun enqueue(context: Context, accountId: Long) {
            val request = OneTimeWorkRequest.Builder(MailFlagWorker::class.java)
                .setInputData(Data.Builder().putLong(KEY_ACCOUNT, accountId).build())
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            // One push per account at a time; a press during a push queues
            // another behind it, which picks up whatever is still dirty.
            WorkManager.getInstance(context)
                .enqueueUniqueWork("mail-flags-$accountId", ExistingWorkPolicy.APPEND_OR_REPLACE, request)
        }
    }
}
