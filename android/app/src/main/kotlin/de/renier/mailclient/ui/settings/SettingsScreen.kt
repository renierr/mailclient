package de.renier.mailclient.ui.settings

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import de.renier.mailclient.MailNative
import de.renier.mailclient.MailNotifier
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.UnsavedChangesDialog
import de.renier.mailclient.ui.common.strings
import de.renier.mailclient.ui.state.MailState
import de.renier.mailclient.ui.state.refreshCapabilities
import de.renier.mailclient.ui.state.settingsSaved
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

private enum class Section(val label: String, val icon: Int) {
    Interface("Interface", R.drawable.ic_palette),
    Mailbox("Mailbox", R.drawable.ic_inbox),
    Reading("Reading", R.drawable.ic_mail),
    Composing("Composing", R.drawable.ic_edit),
    Sync("Accounts & sync", R.drawable.ic_sync),
    Maintenance("Maintenance", R.drawable.ic_save),
    About("About", R.drawable.ic_info),
}

/**
 * Every preference, in sections: a rail beside the form where there is
 * room, tabs above it on a phone. Everything edits a draft; Save writes
 * only what changed (one batch for the app-wide keys, one per account),
 * then re-plans the background checks and asks for what they now need.
 * Close discards, after asking when something changed. Maintenance and
 * About act at once and are not part of the draft.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(state: MailState, onClose: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var section by remember { mutableStateOf(Section.Interface) }
    var draft by remember { mutableStateOf<SettingsDraft?>(null) }
    var choices by remember { mutableStateOf(JSONObject()) }
    // -1: the app-wide sync settings; else one account's overrides.
    var syncScope by remember { mutableStateOf(-1L) }
    val accountDrafts = remember { mutableStateMapOf<Long, AccountDraft>() }
    var saving by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var confirmDiscard by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        val loaded = withContext(Dispatchers.IO) {
            runCatching {
                MailNative.ensureInit(context)
                val c = JSONObject(MailNative.settingChoicesJson())
                c to SettingsDraft(JSONObject(MailNative.settingsJson()), c)
            }
        }
        loaded.onSuccess { (c, d) ->
            choices = c
            draft = d
        }.onFailure { error = it.message ?: "Could not read the settings" }
    }

    LaunchedEffect(syncScope) {
        val id = syncScope
        if (id < 0 || accountDrafts.containsKey(id)) return@LaunchedEffect
        withContext(Dispatchers.IO) { runCatching { AccountDraft(JSONObject(MailNative.accountSettingsJson(id))) } }
            .onSuccess { accountDrafts[id] = it }
            .onFailure { error = it.message }
    }

    val dirty = draft?.dirty == true || accountDrafts.values.any { it.dirty }
    fun maybeClose() {
        if (saving) return
        if (dirty) confirmDiscard = true else onClose()
    }
    BackHandler { maybeClose() }

    val notifyPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    fun testNotification() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            notifyPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
            state.info("Allow notifications, then send the test again")
            return
        }
        MailNotifier.showTest(context)
        state.info("Test notification sent")
    }

    fun save() {
        val d = draft ?: return
        saving = true
        error = null
        val writes = d.changes()
        val sort = if (d.sortChanged) d["message_sort_field"] to d.flag("message_sort_desc") else null
        val perAccount = accountDrafts.mapValues { it.value.changes() }.filterValues { it.isNotEmpty() }
        scope.launch {
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    val before = JSONObject(MailNative.backgroundPlanJson())
                    if (writes.isNotEmpty()) MailNative.setSettings(JSONObject(writes).toString())
                    sort?.let { MailNative.setSort(it.first, it.second) }
                    for ((id, values) in perAccount) MailNative.setAccountSettings(id, JSONObject(values).toString())
                    before to JSONObject(MailNative.backgroundPlanJson())
                }
            }
            result.onSuccess { (before, after) ->
                state.settingsSaved()
                // Background checks just got switched on: Doze postpones a
                // battery-optimised app's checks by hours, so ask for the
                // exemption now (the shell asks for notifications itself).
                if (after.optBoolean("any") && !before.optBoolean("any") && !BackgroundPower.unrestricted(context)) {
                    BackgroundPower.requestUnrestricted(context)
                }
                // Push's keep-alive and the alarm poller want exact alarms.
                if (BackgroundPower.needsExact(after) && !BackgroundPower.needsExact(before) &&
                    !BackgroundPower.exactAlarms(context)
                ) {
                    BackgroundPower.requestExactAlarms(context)
                }
                onClose()
            }.onFailure {
                saving = false
                error = it.message ?: "Could not save the settings"
            }
        }
    }

    Scaffold(
        contentWindowInsets = WindowInsets(0),
        topBar = {
            TopAppBar(
                windowInsets = WindowInsets(0),
                title = { Text("Settings") },
                navigationIcon = {
                    IconButton(onClick = ::maybeClose, enabled = !saving) {
                        Icon(painterResource(R.drawable.ic_close), "Close")
                    }
                },
                actions = {
                    Button(
                        onClick = ::save,
                        enabled = !saving && draft != null,
                        modifier = Modifier.padding(end = 8.dp),
                    ) { Text(if (saving) "Saving…" else "Save") }
                },
            )
        },
    ) { padding ->
        BoxWithConstraints(modifier = Modifier.padding(padding).fillMaxSize()) {
            val rail = maxWidth >= 600.dp && maxHeight >= 480.dp
            val form: @Composable () -> Unit = {
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .verticalScroll(rememberScrollState())
                        .padding(16.dp),
                ) {
                    Column(modifier = Modifier.widthIn(max = 720.dp)) {
                        // Save is in the top bar: its error goes to the top.
                        error?.let {
                            Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(bottom = 8.dp))
                        }
                        val d = draft
                        when {
                            section == Section.Maintenance -> MaintenanceSection(state)
                            section == Section.About -> AboutSection(state)
                            d == null -> CircularProgressIndicator()
                            else -> when (section) {
                                Section.Interface -> InterfaceSection(d)
                                Section.Mailbox -> MailboxSection(d)
                                Section.Reading -> ReadingSection(d)
                                Section.Composing -> ComposingSection(d)
                                Section.Sync -> SyncSection(state, d, choices, syncScope, { syncScope = it }, accountDrafts, ::testNotification)
                                else -> Unit
                            }
                        }
                    }
                }
            }
            if (rail) {
                Row {
                    NavigationRail(modifier = Modifier.fillMaxHeight()) {
                        for (s in Section.entries) {
                            NavigationRailItem(
                                selected = s == section,
                                onClick = { section = s },
                                icon = { Icon(painterResource(s.icon), null) },
                                label = { Text(s.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                            )
                        }
                    }
                    VerticalDivider()
                    form()
                }
            } else {
                Column {
                    PrimaryScrollableTabRow(selectedTabIndex = section.ordinal, edgePadding = 8.dp) {
                        for (s in Section.entries) {
                            Tab(selected = s == section, onClick = { section = s }, text = { Text(s.label) })
                        }
                    }
                    form()
                }
            }
        }
    }

    if (confirmDiscard) {
        UnsavedChangesDialog(
            title = "Discard changes?",
            text = "Your changes to the settings are not saved yet.",
            onSave = {
                confirmDiscard = false
                save()
            },
            onDiscard = {
                confirmDiscard = false
                onClose()
            },
            onDismiss = { confirmDiscard = false },
        )
    }
}

@Composable
private fun Offered(d: SettingsDraft, title: String, key: String, help: String? = null, enabled: Boolean = true) {
    SettingChoice(
        title = title,
        value = d[key],
        options = d.options(key),
        label = { SettingLabels.of(key, it) },
        onChange = { d[key] = it },
        help = help,
        enabled = enabled,
    )
}

@Composable
private fun InterfaceSection(d: SettingsDraft) {
    Offered(d, "Interface scale", "ui_scale")
    Offered(d, "Mail text size", "reader_font_size")
    Offered(
        d,
        "Start in",
        "start_view",
        help = "Where the app opens when started on a narrow screen. Returning from the background keeps what was open.",
    )
}

@Composable
private fun MailboxSection(d: SettingsDraft) {
    Offered(d, "Sort messages by", "message_sort_field")
    SettingChoice(
        title = "Order",
        value = d["message_sort_desc"],
        options = listOf("1", "0"),
        label = { if (it == "1") "Newest first" else "Oldest first" },
        onChange = { d["message_sort_desc"] = it },
    )
    Offered(d, "Density", "list_density")
    SettingSwitch("Confirm before moving mail to Trash", d.flag("confirm_delete"), { d.setFlag("confirm_delete", it) })
}

@Composable
private fun ReadingSection(d: SettingsDraft) {
    SettingSwitch("Automatically mark messages as read", d.flag("auto_mark_read"), { d.setFlag("auto_mark_read", it) })
    Offered(d, "Mark as read", "mark_read_delay_secs", enabled = d.flag("auto_mark_read"))
    SettingSwitch("Load remote images (not recommended)", d.flag("load_remote_images"), { d.setFlag("load_remote_images", it) })
    Offered(d, "Clicking a link in a message", "link_click_action")
}

@Composable
private fun ComposingSection(d: SettingsDraft) {
    Offered(d, "Send mail as", "compose_send_format")
    SettingSwitch(
        "Always include a plain-text version alongside HTML",
        d.flag("compose_include_plain"),
        { d.setFlag("compose_include_plain", it) },
    )
    SettingChoice(
        title = "Replies start",
        value = d["reply_below_quote"],
        options = listOf("0", "1"),
        label = { if (it == "1") "Below the quote" else "Above the quote" },
        onChange = { d["reply_below_quote"] = it },
    )
    SettingSwitch("Use signature", d.flag("signature_enabled"), { d.setFlag("signature_enabled", it) })
    OutlinedTextField(
        value = d["signature_text"],
        onValueChange = { d["signature_text"] = it },
        label = { Text("Signature") },
        minLines = 3,
        modifier = Modifier.fillMaxWidth().padding(vertical = 6.dp),
    )
    SettingSwitch(
        "Offer read receipt",
        d.flag("request_mdn"),
        { d.setFlag("request_mdn", it) },
        help = "Shows a Read receipt toggle in the composer. The recipient's mail app may confirm opening; it only asks.",
    )
    SettingSwitch(
        "Offer delivery confirmation",
        d.flag("request_dsn"),
        { d.setFlag("request_dsn", it) },
        help = "Shows a Delivery confirmation toggle in the composer. The receiving server reports once the mail " +
            "is in the mailbox, where every server supports it. Both toggles start off on each mail.",
    )
}

@Composable
private fun SyncSection(
    state: MailState,
    d: SettingsDraft,
    choices: JSONObject,
    scope: Long,
    onScope: (Long) -> Unit,
    accountDrafts: Map<Long, AccountDraft>,
    onTestNotification: () -> Unit,
) {
    val accounts = state.accounts
    val shown = if (accounts.any { it.id == scope }) scope else -1L
    if (accounts.isNotEmpty()) {
        SettingChoice(
            title = "Settings for",
            value = shown.toString(),
            options = listOf("-1") + accounts.map { it.id.toString() },
            label = { id -> if (id == "-1") "All accounts" else accounts.firstOrNull { it.id.toString() == id }?.email ?: id },
            onChange = { onScope(it.toLong()) },
            help = if (shown < 0) "Accounts use these unless they set their own." else null,
        )
    }
    if (shown < 0) {
        GlobalSyncSettings(d, choices, onTestNotification)
        BackgroundStatus()
    } else {
        val a = accountDrafts[shown]
        if (a == null) CircularProgressIndicator() else AccountSyncSettings(d, a, choices)
    }
}

/** Version, licence, database path, and an account's server capabilities. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun AboutSection(state: MailState) {
    val info = remember { runCatching { JSONObject(MailNative.appInfoJson()) }.getOrDefault(JSONObject()) }
    var accountId by remember { mutableStateOf(state.activeAccountId) }
    // Like Qt and Flutter: load the open account's list on first visit.
    LaunchedEffect(Unit) {
        if (accountId >= 0 && state.capabilities[accountId] == null) state.refreshCapabilities(accountId)
    }
    SelectionContainer {
        Column {
            Text("Mailclient ${info.optString("version")}")
            Text("License: ${info.optString("license")}")
            Text("Database: ${info.optString("db_path")}")
        }
    }
    SettingHeading("Server capabilities")
    val accounts = state.accounts
    if (accounts.isEmpty()) {
        Text("Add an account first")
        return
    }
    SettingChoice(
        title = "Account",
        value = accountId.toString(),
        options = accounts.map { it.id.toString() },
        label = { id -> accounts.firstOrNull { it.id.toString() == id }?.email ?: id },
        onChange = {
            accountId = it.toLong()
            if (state.capabilities[accountId] == null) state.refreshCapabilities(accountId)
        },
    )
    OutlinedButton(onClick = { state.refreshCapabilities(accountId) }, enabled = accountId >= 0) { Text("Refresh") }
    val caps = state.capabilities[accountId]
    // Only this job's own state: a sync running elsewhere is not "loading".
    val loading = "Capabilities" in state.busyKinds
    val failure = state.capabilitiesError[accountId]
    if (!loading && failure != null) {
        Text(
            failure,
            color = MaterialTheme.colorScheme.error,
            modifier = Modifier.padding(top = 8.dp),
        )
    }
    if (caps == null || loading) {
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 8.dp)) {
            if (loading) CircularProgressIndicator(strokeWidth = 2.dp, modifier = Modifier.size(14.dp))
            Text(
                when {
                    loading -> "Loading capabilities…"
                    failure != null -> ""
                    else -> "No capabilities loaded yet — press Refresh."
                },
                modifier = Modifier.padding(start = 8.dp),
            )
        }
    }
    if (caps != null) {
        SelectionContainer {
            Text(
                "${caps.optString("email")} · ${caps.optString("imap_host")}",
                modifier = Modifier.padding(vertical = 8.dp),
            )
        }
        val list = remember(caps) { caps.optJSONArray("capabilities").strings() }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            for (cap in list) {
                AssistChip(onClick = {}, label = { Text(cap, fontFamily = FontFamily.Monospace) })
            }
        }
    }
}
