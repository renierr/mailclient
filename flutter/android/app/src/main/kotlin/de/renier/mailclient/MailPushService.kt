package de.renier.mailclient

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.os.Build
import android.os.IBinder
import android.os.PowerManager

// Push mail: keeps the Rust IMAP IDLE monitor (mailcore::sync::push) alive in
// a foreground service, the only way Android lets an app hold connections
// while it is closed.
//
// The service itself does almost nothing: the monitor runs on its own
// thread and calls back here. onBusy holds a wake lock exactly while the
// monitor works, so the CPU sleeps whenever every account waits in IDLE.
// A network change reconnects every account at once; the keep-alive alarm
// (MailPush) refreshes the connections while the phone sleeps.
//
// Android requires a visible notification for a foreground service. It
// goes to its own "Mail monitor" channel at the lowest importance: no status
// bar icon, no sound, one collapsed line in the shade — and the channel can
// be switched off in the system settings without stopping push.
class MailPushService : Service(), PushCallbacks {
    private lateinit var wakeLock: PowerManager.WakeLock
    private var networkCallback: ConnectivityManager.NetworkCallback? = null

    override fun onCreate() {
        super.onCreate()
        goForeground()
        running = true
        MailNative.ensureInit(this)
        val power = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "mailclient:push")
            .apply { setReferenceCounted(false) }
        val connectivity = getSystemService(ConnectivityManager::class.java)
        MailNative.pushStart(this, connectivity?.activeNetwork != null)
        watchNetwork(connectivity)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Every startForegroundService() needs its own startForeground().
        goForeground()
        return START_STICKY
    }

    override fun onDestroy() {
        running = false
        networkCallback?.let { cb ->
            getSystemService(ConnectivityManager::class.java)?.unregisterNetworkCallback(cb)
        }
        networkCallback = null
        MailNative.pushStop()
        if (wakeLock.isHeld) wakeLock.release()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onBusy(busy: Boolean) {
        // Bounded, so a monitor that never reports back cannot keep the
        // phone awake for good.
        if (busy) wakeLock.acquire(BUSY_TIMEOUT_MS) else if (wakeLock.isHeld) wakeLock.release()
    }

    override fun onReport(report: String) {
        MailNotifier.deliver(this, report)
    }

    // Reconnect only when the default network really changes: the callback
    // reports the current network once on registration, too.
    private fun watchNetwork(connectivity: ConnectivityManager?) {
        if (connectivity == null) return
        val callback = object : ConnectivityManager.NetworkCallback() {
            private var current: Network? = connectivity.activeNetwork

            override fun onAvailable(network: Network) {
                if (network == current) return
                current = network
                MailNative.pushNetwork(true)
            }

            override fun onLost(network: Network) {
                if (network != current) return
                current = null
                MailNative.pushNetwork(false)
            }
        }
        connectivity.registerDefaultNetworkCallback(callback)
        networkCallback = callback
    }

    private fun goForeground() {
        val notification = monitorNotification()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION_ID, notification)
        }
    }

    @Suppress("DEPRECATION")
    private fun monitorNotification(): Notification {
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val manager = getSystemService(NotificationManager::class.java)
            if (manager.getNotificationChannel(CHANNEL_ID) == null) {
                manager.createNotificationChannel(
                    NotificationChannel(CHANNEL_ID, "Mail monitor", NotificationManager.IMPORTANCE_MIN).apply {
                        description = "Keeps push mail connected. Safe to turn off: push keeps working."
                        setShowBadge(false)
                    },
                )
            }
            Notification.Builder(this, CHANNEL_ID)
        } else {
            Notification.Builder(this).setPriority(Notification.PRIORITY_MIN)
        }
        val open = PendingIntent.getActivity(
            this,
            1,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        return builder
            .setSmallIcon(R.drawable.ic_launcher_monochrome)
            .setContentTitle("Push mail is on")
            .setOngoing(true)
            .setShowWhen(false)
            .setCategory(Notification.CATEGORY_SERVICE)
            .setContentIntent(open)
            .build()
    }

    companion object {
        // Whether the service runs in this process (MailPush.tick).
        @Volatile var running = false

        private const val CHANNEL_ID = "mail_push"
        private const val NOTIFICATION_ID = 2
        private const val BUSY_TIMEOUT_MS = 3 * 60_000L
    }
}
