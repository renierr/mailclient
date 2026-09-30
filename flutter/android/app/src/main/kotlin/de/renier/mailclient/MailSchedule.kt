package de.renier.mailclient

import android.content.Context

// Which background mechanism checks for mail while the app is closed, from
// the `background_scheduler` setting: exactly one runs, the others are
// stopped, so two never check side by side. An interval of 0 (checking
// manually) stops all of them.
object MailSchedule {
    fun apply(context: Context, mode: String, minutes: Int) {
        val on = minutes > 0
        if (on && mode == "push") MailPush.enable(context) else MailPush.disable(context)
        if (on && mode == "alarm") MailAlarm.arm(context, minutes) else MailAlarm.cancel(context)
        if (on && mode != "push" && mode != "alarm") {
            MailCheckWorker.schedulePeriodic(context, minutes)
        } else {
            MailCheckWorker.cancelPeriodic(context)
        }
    }
}
