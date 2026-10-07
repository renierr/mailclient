package de.renier.mailclient

import android.app.Application

// Installs the crash capture before any activity, service or receiver
// runs: every one of them starts with Application.onCreate in the same
// process, so a single install here covers the composer, the background
// worker and the push service. Reports stay on-device under `crashes/`
// until shared from Settings → Maintenance.
class MailApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        CrashLog.install(this)
    }
}
