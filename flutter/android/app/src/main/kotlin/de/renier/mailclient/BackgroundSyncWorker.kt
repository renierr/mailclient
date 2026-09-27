package de.renier.mailclient

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.embedding.engine.dart.DartExecutor
import io.flutter.plugin.common.MethodChannel
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class BackgroundSyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        return withContext(Dispatchers.IO) {
            val engine = FlutterEngine(applicationContext)
            try {
                engine.dartExecutor.executeDartEntrypoint(
                    DartExecutor.DartEntrypoint.createDefault()
                )
                val latch = CountDownLatch(1)
                var success = false
                MethodChannel(engine.dartExecutor.binaryMessenger, "mailclient/background_sync")
                    .setMethodCallHandler { call, result ->
                        if (call.method == "runBackgroundCheck") {
                            success = result.success(true)
                        } else {
                            result.notImplemented()
                        }
                        latch.countDown()
                    }
                latch.await(60, TimeUnit.SECONDS)
                engine.destroy()
                if (success) Result.success() else Result.retry()
            } catch (e: Exception) {
                engine.destroy()
                Result.retry()
            }
        }
    }
}
