package de.renier.mailclient

import android.app.PendingIntent
import android.app.RemoteInput
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.util.Log
import androidx.work.Constraints
import androidx.work.Data
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequest
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters

// The notifications' buttons: "Mark read" / "Mark all read", "Archive" or
// "Delete", and "Reply". Each changes the cache at once and re-plans the
// notifications (the mail drops out of them), then leaves the network to a
// worker that waits for a connection: MailFlagWorker pushes read flags and
// moves, MailReplyWorker sends the reply.
class MailActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val target = intent.getStringExtra(EXTRA_TARGET) ?: return
        val app = context.applicationContext
        val reply = if (intent.action == ACTION_REPLY) {
            RemoteInput.getResultsFromIntent(intent)?.getCharSequence(KEY_REPLY)?.toString()
        } else {
            null
        }
        val pending = goAsync()
        Thread {
            try {
                MailNative.ensureInit(app)
                val account = MailNative.readTargetAccount(target)
                when (intent.action) {
                    ACTION_MARK_READ -> {
                        MailNotifier.deliver(app, MailNative.markRead(target))
                        MailFlagWorker.enqueue(app, account)
                    }
                    ACTION_ACT -> {
                        val action = intent.getStringExtra(EXTRA_ACTION).orEmpty()
                        MailNotifier.deliver(app, MailNative.notifyAct(target, action))
                        MailFlagWorker.enqueue(app, account)
                    }
                    ACTION_REPLY -> if (!reply.isNullOrBlank()) {
                        // The notification must change at once (Android shows
                        // a spinner until it does): answered means read, so
                        // it leaves. The send waits for a network.
                        MailNotifier.deliver(app, MailNative.markRead(target))
                        MailReplyWorker.enqueue(app, target, reply, intent.getStringExtra(EXTRA_OPEN).orEmpty())
                        MailFlagWorker.enqueue(app, account)
                    }
                }
                MailNotifier.onMailChanged?.invoke()
            } catch (e: RuntimeException) {
                Log.w("mailclient", "notification action ${intent.action} failed", e)
            } finally {
                pending.finish()
            }
        }.start()
    }

    companion object {
        private const val ACTION_MARK_READ = "de.renier.mailclient.MARK_READ"
        private const val ACTION_ACT = "de.renier.mailclient.NOTIFY_ACT"
        private const val ACTION_REPLY = "de.renier.mailclient.NOTIFY_REPLY"
        private const val EXTRA_TARGET = "target"
        private const val EXTRA_ACTION = "action"
        private const val EXTRA_OPEN = "open"
        const val KEY_REPLY = "reply_text"

        // One PendingIntent per notification and button (tag and action go
        // into the data URI), each carrying the ReadTarget JSON its plan gave
        // it.
        fun markReadIntent(context: Context, tag: String, target: String): PendingIntent =
            broadcast(context, actionIntent(context, ACTION_MARK_READ, tag, target), mutable = false)

        fun actIntent(context: Context, tag: String, target: String, action: String): PendingIntent =
            broadcast(context, actionIntent(context, ACTION_ACT, tag, target).putExtra(EXTRA_ACTION, action), mutable = false)

        // Mutable: Android adds the typed text to the intent. The tag is the
        // mail's open payload, so a failed send can link back to the mail.
        fun replyIntent(context: Context, tag: String, target: String): PendingIntent =
            broadcast(context, actionIntent(context, ACTION_REPLY, tag, target).putExtra(EXTRA_OPEN, tag), mutable = true)

        private fun actionIntent(context: Context, action: String, tag: String, target: String): Intent =
            Intent(context, MailActionReceiver::class.java)
                .setAction(action)
                .setData(Uri.fromParts("mailclient", tag, action))
                .putExtra(EXTRA_TARGET, target)

        private fun broadcast(context: Context, intent: Intent, mutable: Boolean): PendingIntent {
            val flag = if (mutable && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                PendingIntent.FLAG_MUTABLE
            } else if (mutable) {
                0
            } else {
                PendingIntent.FLAG_IMMUTABLE
            }
            return PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT or flag)
        }
    }
}

// Carries read flags and moves made from a notification to the server, over
// a fresh connection. WorkManager holds it until a network is up and retries
// a failed push; a sync pushes anything still queued anyway.
class MailFlagWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    override fun doWork(): Result = try {
        MailNative.ensureInit(applicationContext)
        MailNative.pushChanges(inputData.getLong(KEY_ACCOUNT, 0))
        Result.success()
    } catch (e: RuntimeException) {
        Log.w("mailclient", "change push failed", e)
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

// Sends a notification Reply once a network is up. Never retried: a failed
// submit may still have reached the server, and the core leaves a retry to
// the user (the outbox shows the failure). The text is kept in a "Reply not
// sent" notification instead.
class MailReplyWorker(context: Context, params: WorkerParameters) : Worker(context, params) {
    override fun doWork(): Result {
        val target = inputData.getString(KEY_TARGET) ?: return Result.failure()
        val text = inputData.getString(KEY_TEXT).orEmpty()
        val open = inputData.getString(KEY_OPEN).orEmpty()
        return try {
            MailNative.ensureInit(applicationContext)
            MailNative.notifyReply(target, text)
            MailNotifier.onMailChanged?.invoke()
            Result.success()
        } catch (e: RuntimeException) {
            Log.w("mailclient", "notification reply not sent", e)
            MailNotifier.showReplyFailed(applicationContext, open, text, e.message ?: "The reply could not be sent")
            Result.failure()
        }
    }

    companion object {
        private const val KEY_TARGET = "target"
        private const val KEY_TEXT = "text"
        private const val KEY_OPEN = "open"

        fun enqueue(context: Context, target: String, text: String, open: String) {
            val request = OneTimeWorkRequest.Builder(MailReplyWorker::class.java)
                .setInputData(
                    Data.Builder()
                        .putString(KEY_TARGET, target)
                        .putString(KEY_TEXT, text)
                        .putString(KEY_OPEN, open)
                        .build(),
                )
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context).enqueue(request)
        }
    }
}
