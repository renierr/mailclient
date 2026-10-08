package de.renier.mailclient.ui.settings

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import de.renier.mailclient.CrashLog
import de.renier.mailclient.MailNative
import de.renier.mailclient.ui.state.MailState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File

private class Confirm(val title: String, val text: String, val action: String, val run: () -> String)

/**
 * Storage stats and one-shot local actions: database export, temp cleanup,
 * downloaded-file eviction and cache trimming (`mailcore::maintenance`).
 * Not part of the settings draft: the stats are a live read and every
 * action applies at once, after a confirm. Nothing here touches the server.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun MaintenanceSection(state: MailState) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // The reader's viewer copies, where ReaderFiles stages them.
    val tempDir = remember { File(context.cacheDir, "mailclient-attachments").path }
    val dbPath = remember {
        runCatching { JSONObject(MailNative.appInfoJson()).optString("db_path") }.getOrDefault("")
    }
    var stats by remember { mutableStateOf<JSONObject?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var acting by remember { mutableStateOf(false) }
    var confirm by remember { mutableStateOf<Confirm?>(null) }

    LaunchedEffect(reload) {
        stats = withContext(Dispatchers.IO) {
            runCatching { JSONObject(MailNative.storageStatsJson(dbPath, tempDir)) }.getOrNull()
        }
    }

    // Run [work] off the main thread; its words go to the snackbar.
    fun act(work: () -> String) {
        acting = true
        scope.launch {
            val result = withContext(Dispatchers.IO) { runCatching(work) }
            acting = false
            state.info(result.getOrElse { it.message ?: "Failed" })
            reload++
        }
    }

    // A consistent snapshot into the app cache, then copied where the user
    // picked: the core writes paths, the picker hands out a content Uri.
    // One left behind by a killed process goes in MailApplication's sweep.
    val exportPicker = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/vnd.sqlite3"),
    ) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        act {
            val tmp = File(context.cacheDir, "export-${System.currentTimeMillis()}.sqlite")
            try {
                MailNative.exportDatabaseTo(tmp.path)
                context.contentResolver.openOutputStream(uri)?.use { out -> tmp.inputStream().use { it.copyTo(out) } }
                    ?: error("Could not write the backup")
                "Database exported"
            } finally {
                tmp.delete()
            }
        }
    }

    // Crash reports (CrashLog): uncaught exceptions the app wrote before
    // dying, for installs without adb. Nothing leaves the device except
    // through Share/Save below.
    var crashes by remember { mutableStateOf<List<File>>(emptyList()) }
    var crashReload by remember { mutableIntStateOf(0) }
    var viewing by remember { mutableStateOf<Pair<String, String>?>(null) }
    var saveTarget by remember { mutableStateOf<File?>(null) }
    LaunchedEffect(crashReload) {
        crashes = withContext(Dispatchers.IO) { CrashLog.list(context) }
    }
    fun shareCrash(file: File) {
        scope.launch {
            val text = withContext(Dispatchers.IO) { runCatching { file.readText() }.getOrNull() }
            if (text == null) {
                state.info("Could not read the crash report")
            } else {
                val send = Intent(Intent.ACTION_SEND).setType("text/plain")
                    .putExtra(Intent.EXTRA_SUBJECT, "mailclient ${file.name}")
                    .putExtra(Intent.EXTRA_TEXT, text)
                context.startActivity(Intent.createChooser(send, "Share crash report"))
            }
        }
    }
    val crashSaver = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("text/plain"),
    ) { uri ->
        val target = saveTarget
        saveTarget = null
        if (uri == null || target == null) return@rememberLauncherForActivityResult
        act {
            context.contentResolver.openOutputStream(uri)?.use { out ->
                target.inputStream().use { it.copyTo(out) }
            } ?: error("Could not write the crash report")
            "Crash report saved"
        }
    }

    Column {
        SettingHeading("Storage")
        val s = stats
        if (s == null) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                CircularProgressIndicator(strokeWidth = 2.dp, modifier = Modifier.size(14.dp))
                Text("Loading storage…", modifier = Modifier.padding(start = 8.dp))
            }
        } else {
            SelectionContainer {
                Column {
                    Text("Database: ${s.optString("db_display")}")
                    Text("${s.optLong("message_count")} messages cached")
                    Text("Downloaded files: ${s.optString("cached_display")}")
                    Text("Temporary files: ${s.optString("temp_display")}")
                    Text(
                        s.optString("db_path"),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        OutlinedButton(onClick = { reload++ }, enabled = !acting, modifier = Modifier.padding(top = 8.dp)) {
            Text("Refresh")
        }

        SettingHeading("Database backup")
        Text("Saves a consistent copy of the local mail database. Your mail stays where it is.")
        OutlinedButton(
            onClick = { exportPicker.launch("mailclient-backup.sqlite") },
            enabled = !acting,
            modifier = Modifier.padding(top = 8.dp),
        ) { Text("Export database…") }

        SettingHeading("Cache")
        Text(
            "Frees local space only — nothing here touches the mail server. Trimmed messages return with the " +
                "next sync; removed files download again when opened.",
        )
        val keep = stats?.optInt("keep_per_folder", 200) ?: 200
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.padding(top = 8.dp),
        ) {
            OutlinedButton(enabled = !acting, onClick = {
                confirm = Confirm(
                    "Clean temporary files?",
                    "Delete staged viewer copies? They are re-created the next time an attachment is opened. " +
                        "Mail on the server is untouched.",
                    "Clean",
                ) { JSONObject(MailNative.cleanupTempFilesJson(tempDir)).optString("status") }
            }) { Text("Clean temporary files") }
            OutlinedButton(enabled = !acting, onClick = {
                confirm = Confirm(
                    "Remove downloaded files?",
                    "Delete downloaded attachment files from this device? Names and sizes stay, and files " +
                        "download again when opened. Mail on the server is untouched.",
                    "Remove",
                ) { JSONObject(MailNative.evictCachedAttachmentsJson()).optString("status") }
            }) { Text("Remove downloaded files") }
            OutlinedButton(enabled = !acting, onClick = {
                confirm = Confirm(
                    "Trim old messages?",
                    "Delete cached messages past the newest $keep per folder from this device? Drafts and " +
                        "unsent mail are kept, and trimmed mail returns with the next sync. Mail on the server " +
                        "is untouched.",
                    "Trim",
                ) {
                    val removed = MailNative.trimLocalCache().toLongOrNull() ?: 0L
                    MailNative.trimStatus(removed)
                }
            }) { Text("Trim old messages") }
        }

        SettingHeading("Crash reports")
        Text(
            "When the app closes unexpectedly it writes a report here: app version, device and the " +
                "stack trace. No mail content, no passwords — share one when a crash is investigated.",
        )
        if (crashes.isEmpty()) {
            Text("No crash reports saved.", modifier = Modifier.padding(top = 8.dp))
        } else {
            Column(modifier = Modifier.padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                for (report in crashes) {
                    Column(modifier = Modifier.fillMaxWidth()) {
                        Text(CrashLog.describe(report), style = MaterialTheme.typography.bodySmall)
                        FlowRow(
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            OutlinedButton(onClick = {
                                scope.launch {
                                    val text = withContext(Dispatchers.IO) {
                                        runCatching { report.readText() }.getOrNull()
                                            ?: "Could not read the crash report"
                                    }
                                    viewing = report.name to text
                                }
                            }) { Text("View") }
                            OutlinedButton(onClick = { shareCrash(report) }) { Text("Share") }
                            OutlinedButton(onClick = {
                                saveTarget = report
                                crashSaver.launch(report.name)
                            }) { Text("Save…") }
                        }
                    }
                }
            }
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.padding(top = 8.dp),
            ) {
                OutlinedButton(enabled = !acting, onClick = { crashReload++ }) { Text("Refresh") }
                OutlinedButton(enabled = !acting, onClick = {
                    confirm = Confirm(
                        "Delete crash reports?",
                        "Delete all ${crashes.size} saved crash report(s) from this device?",
                        "Delete",
                    ) {
                        CrashLog.deleteAll(context)
                        crashReload++
                        "Crash reports deleted"
                    }
                }) { Text("Delete all") }
            }
        }
    }

    viewing?.let { (name, text) ->
        AlertDialog(
            onDismissRequest = { viewing = null },
            title = { Text(name) },
            text = {
                SelectionContainer {
                    Text(
                        text,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.verticalScroll(rememberScrollState()).heightIn(max = 400.dp),
                    )
                }
            },
            confirmButton = { TextButton(onClick = { viewing = null }) { Text("Close") } },
        )
    }

    confirm?.let { c ->
        AlertDialog(
            onDismissRequest = { confirm = null },
            title = { Text(c.title) },
            text = { Text(c.text) },
            confirmButton = {
                Button(
                    onClick = {
                        confirm = null
                        act(c.run)
                    },
                    colors = ButtonDefaults.buttonColors(
                        containerColor = MaterialTheme.colorScheme.error,
                        contentColor = MaterialTheme.colorScheme.onError,
                    ),
                ) { Text(c.action) }
            },
            dismissButton = { TextButton(onClick = { confirm = null }) { Text("Cancel") } },
        )
    }
}
