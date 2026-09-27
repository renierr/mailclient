package de.renier.mailclient

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkManager
import androidx.work.Worker
import androidx.work.WorkerParameters
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.util.concurrent.TimeUnit

/// Hosts the one native call the notification plugins do not cover:
/// the Android 13+ runtime prompt for `POST_NOTIFICATIONS`
/// (`flutter_local_notifications` 8.x only requests on iOS).
class MainActivity : FlutterActivity() {
    private var pendingPermissionResult: MethodChannel.Result? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "mailclient/permissions")
            .setMethodCallHandler { call, result ->
                if (call.method == "requestNotifications") {
                    if (Build.VERSION.SDK_INT < 33 ||
                        ContextCompat.checkSelfPermission(
                            this,
                            Manifest.permission.POST_NOTIFICATIONS,
                        ) == PackageManager.PERMISSION_GRANTED
                    ) {
                        result.success(true)
                    } else {
                        pendingPermissionResult = result
                        ActivityCompat.requestPermissions(
                            this,
                            arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                            1001,
                        )
                    }
                } else if (call.method == "enqueueExpedited") {
                    val intervalMinutes = call.argument<Int>("intervalMinutes") ?: 15
                    val constraints = Constraints.Builder()
                        .setRequiredNetworkType(NetworkType.CONNECTED)
                        .build()
                    val workRequest = OneTimeWorkRequestBuilder<BackgroundSyncWorker>()
                        .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
                        .setConstraints(constraints)
                        .setInitialDelay(intervalMinutes.toLong(), TimeUnit.MINUTES)
                        .addTag("mail-background-sync")
                        .build()
                    WorkManager.getInstance(this).enqueueUniqueWork(
                        "mail-background-sync",
                        ExistingWorkPolicy.REPLACE,
                        workRequest
                    )
                    result.success(true)
                } else if (call.method == "cancelExpedited") {
                    WorkManager.getInstance(this).cancelUniqueWork("mail-background-sync")
                    result.success(true)
                } else {
                    result.notImplemented()
                }
            }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == 1001) {
            pendingPermissionResult?.success(
                grantResults.isNotEmpty() &&
                    grantResults[0] == PackageManager.PERMISSION_GRANTED,
            )
            pendingPermissionResult = null
        }
    }
}
