package de.renier.mailclient

import android.content.Context

// Which background mechanisms check for mail while the app is closed, as
// the Rust core planned them from every account's settings
// (mailcore::sync::background::schedule): the push service when any account
// uses push, and one poller (WorkManager or the exact alarm) every `minutes`
// for the polled accounts. Each poller tick only checks the accounts that
// are due, so the two never check the same account. Everything not in the
// plan is stopped.
object MailSchedule {
    fun apply(context: Context, push: Boolean, pollMode: String, minutes: Int) {
        if (push) MailPush.enable(context) else MailPush.disable(context)
        val poll = minutes > 0
        if (poll && pollMode == "alarm") MailAlarm.arm(context, minutes) else MailAlarm.cancel(context)
        if (poll && pollMode != "alarm") {
            MailCheckWorker.schedulePeriodic(context, minutes)
        } else {
            MailCheckWorker.cancelPeriodic(context)
        }
    }
}
