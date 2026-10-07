package de.renier.mailclient.ui.accounts

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.ui.state.MailState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

// Step 2 account setup (add + edit): identity, IMAP and SMTP with the core's
// guesses, port-follow, per-field check and save. Blank password on edit
// keeps the stored secret — passwords are write-only. Dirty-guarded close.
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun AccountSetupScreen(
    state: MailState,
    accountId: Long,
    onSaved: () -> Unit,
    onClose: () -> Unit,
) {
    val editing = accountId >= 0
    val scope = rememberCoroutineScope()
    var loaded by remember { mutableStateOf(false) }
    var email by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var fromName by remember { mutableStateOf("") }
    var imapHost by remember { mutableStateOf("") }
    var imapPort by remember { mutableStateOf("") }
    var imapSec by remember { mutableStateOf("tls") }
    var imapUser by remember { mutableStateOf("") }
    var imapPassword by remember { mutableStateOf("") }
    var smtpHost by remember { mutableStateOf("") }
    var smtpPort by remember { mutableStateOf("") }
    var smtpSec by remember { mutableStateOf("tls") }
    var smtpUser by remember { mutableStateOf("") }
    var smtpPassword by remember { mutableStateOf("") }
    var showImapPassword by remember { mutableStateOf(false) }
    var showSmtpPassword by remember { mutableStateOf(false) }
    var touchedHosts by remember { mutableStateOf(false) }
    // What the address guess last filled in: a field still holding it may
    // follow the next guess, one the user typed never does (Qt fills only
    // empty fields).
    var guessed by remember { mutableStateOf(mapOf<String, String>()) }
    // The core's offered values, in display order (`SECURITY_CHOICES`).
    var securityChoices by remember { mutableStateOf(listOf("tls", "starttls", "none")) }
    var errors by remember { mutableStateOf(mapOf<String, String>()) }
    var warnings by remember { mutableStateOf(mapOf<String, String>()) }
    var saveError by remember { mutableStateOf<String?>(null) }
    var loadError by remember { mutableStateOf<String?>(null) }
    var saving by remember { mutableStateOf(false) }
    var testing by remember { mutableStateOf(false) }
    var testResult by remember { mutableStateOf<ConnTest?>(null) }
    var confirmDiscard by remember { mutableStateOf(false) }
    var baseline by remember { mutableStateOf("") }

    fun formJson(): String = JSONObject()
        .put("id", accountId)
        .put("name", name)
        .put("email", email)
        .put("from_name", fromName)
        .put("imap_host", imapHost)
        .put("imap_port", imapPort)
        .put("imap_sec", imapSec)
        .put("imap_user", imapUser)
        .put("password", imapPassword)
        .put("smtp_host", smtpHost)
        .put("smtp_port", smtpPort)
        .put("smtp_sec", smtpSec)
        .put("smtp_user", smtpUser)
        .put("smtp_password", smtpPassword)
        .toString()

    // Passwords never enter the dirty snapshot: typing one must not flip
    // the guard, and the baseline never holds one.
    fun snapshot(): String = JSONObject(formJson())
        .put("password", "")
        .put("smtp_password", "")
        .toString()

    // New forms start from the core defaults; edits load the stored form
    // (never a password). The shell already initialized the core.
    LaunchedEffect(accountId) {
        withContext(Dispatchers.IO) {
            val result = runCatching {
                if (editing) MailNative.accountForm(accountId)
                else MailNative.accountFormDefaults()
            }
            val choices = runCatching {
                val a = JSONObject(MailNative.accountFormDefaults()).getJSONArray("security_choices")
                List(a.length()) { a.getString(it) }
            }.getOrNull()
            withContext(Dispatchers.Main) {
                if (!choices.isNullOrEmpty()) securityChoices = choices
                result
                    .onSuccess { form ->
                        val o = JSONObject(form)
                        email = o.optString("email")
                        name = o.optString("name")
                        fromName = o.optString("from_name")
                        imapHost = o.optString("imap_host")
                        imapPort = o.optString("imap_port")
                        imapSec = o.optString("imap_sec", "tls")
                        imapUser = o.optString("imap_user")
                        smtpHost = o.optString("smtp_host")
                        smtpPort = o.optString("smtp_port")
                        smtpSec = o.optString("smtp_sec", "tls")
                        smtpUser = o.optString("smtp_user")
                        baseline = snapshot()
                        loaded = true
                    }
                    .onFailure { loadError = it.message }
            }
        }
    }

    // Inline check on every change (fast sync core call, off the UI thread).
    // A changed field also invalidates the last connection test.
    fun recheck() {
        testResult = null
        val form = formJson()
        val ed = editing
        scope.launch(Dispatchers.IO) {
            val check = JSONObject(MailNative.accountFormCheck(form, ed))
            val errs = mutableMapOf<String, String>()
            val warns = mutableMapOf<String, String>()
            check.optJSONObject("errors")?.keys()?.forEach { k ->
                errs[k] = check.optJSONObject("errors")?.optString(k) ?: ""
            }
            check.optJSONObject("warnings")?.keys()?.forEach { k ->
                warns[k] = check.optJSONObject("warnings")?.optString(k) ?: ""
            }
            withContext(Dispatchers.Main) {
                errors = errs
                warnings = warns
            }
        }
    }

    fun guessFor(typed: String) {
        if (touchedHosts || editing) return
        scope.launch(Dispatchers.IO) {
            val g = JSONObject(MailNative.accountGuess(typed))
            if (g.length() == 0) return@launch
            withContext(Dispatchers.Main) {
                if (!touchedHosts) {
                    fun follow(key: String, current: String): String =
                        if (current.isEmpty() || current == guessed[key]) g.optString(key, current) else current
                    imapHost = follow("imap_host", imapHost)
                    smtpHost = follow("smtp_host", smtpHost)
                    imapUser = follow("imap_user", imapUser)
                    guessed = mapOf(
                        "imap_host" to g.optString("imap_host"),
                        "smtp_host" to g.optString("smtp_host"),
                        "imap_user" to g.optString("imap_user"),
                    )
                    recheck()
                }
            }
        }
    }

    fun securityChanged(protocol: String, next: String) {
        scope.launch(Dispatchers.IO) {
            if (protocol == "imap") {
                val port = MailNative.accountPortForSecurity(protocol, imapSec, next, imapPort)
                withContext(Dispatchers.Main) {
                    imapSec = next
                    imapPort = port
                    recheck()
                }
            } else {
                val port = MailNative.accountPortForSecurity(protocol, smtpSec, next, smtpPort)
                withContext(Dispatchers.Main) {
                    smtpSec = next
                    smtpPort = port
                    recheck()
                }
            }
        }
    }

    fun save() {
        saveError = null
        saving = true
        val form = formJson()
        val ed = editing
        scope.launch(Dispatchers.IO) {
            val result = runCatching {
                // Surface the core check first so the composer-style inline
                // path and the save path agree.
                val check = JSONObject(MailNative.accountFormCheck(form, ed))
                val first = check.optJSONObject("errors")?.keys()?.asSequence()?.firstOrNull()
                if (first != null) {
                    throw IllegalArgumentException(
                        check.optJSONObject("errors")?.optString(first) ?: "invalid form",
                    )
                }
                MailNative.saveAccount(form)
            }
            withContext(Dispatchers.Main) {
                saving = false
                result
                    .onSuccess { id ->
                        val accountId = id.toLong()
                        state.selectAccount(accountId)
                        state.refreshAll()
                        // A fresh account has nothing cached: sync at once so
                        // the list fills with visible progress instead of an
                        // empty "pull to sync" screen.
                        state.syncAccount(accountId)
                        onSaved()
                    }
                    .onFailure { saveError = it.message }
            }
        }
    }

    // Live IMAP + SMTP login check (slow network call, off the UI thread).
    // Catches a mistyped hostname before saving; saving itself stays
    // offline-safe (no network required).
    fun testConnection() {
        saveError = null
        testing = true
        testResult = null
        val form = formJson()
        scope.launch(Dispatchers.IO) {
            val result = runCatching { parseConnTest(MailNative.testAccountConnection(form)) }
            withContext(Dispatchers.Main) {
                testing = false
                result
                    .onSuccess { testResult = it }
                    .onFailure { saveError = it.message }
            }
        }
    }

    val dirty = loaded && snapshot() != baseline
    BackHandler(enabled = dirty && !confirmDiscard) { confirmDiscard = true }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Section("Identity")
        Field("Email", email, errors["email"], warnings["email"], KeyboardType.Email) {
            email = it
            guessFor(it)
            recheck()
        }
        Field("Display name", name, errors["name"], warnings["name"]) {
            name = it
            recheck()
        }
        Field("Sender name", fromName, errors["from_name"], warnings["from_name"]) {
            fromName = it
            recheck()
        }

        Section("IMAP")
        Field("Host", imapHost, errors["imap_host"], warnings["imap_host"]) {
            imapHost = it
            touchedHosts = true
            recheck()
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Field(
                "Port", imapPort, errors["imap_port"], warnings["imap_port"],
                KeyboardType.Number, Modifier.weight(1f),
            ) {
                imapPort = it
                recheck()
            }
            SecurityPicker("Security", imapSec, securityChoices, Modifier.weight(1f)) {
                securityChanged("imap", it)
            }
        }
        SecurityWarning(warnings["imap_sec"])
        Field("Username", imapUser, errors["imap_user"], warnings["imap_user"]) {
            imapUser = it
            recheck()
        }
        PasswordField(
            "Password" + if (editing) " (blank keeps stored)" else "",
            imapPassword, errors["password"], warnings["password"], showImapPassword,
            onReveal = { showImapPassword = !showImapPassword },
        ) {
            imapPassword = it
            recheck()
        }

        Section("SMTP")
        Field("Host", smtpHost, errors["smtp_host"], warnings["smtp_host"]) {
            smtpHost = it
            touchedHosts = true
            recheck()
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Field(
                "Port", smtpPort, errors["smtp_port"], warnings["smtp_port"],
                KeyboardType.Number, Modifier.weight(1f),
            ) {
                smtpPort = it
                recheck()
            }
            SecurityPicker("Security", smtpSec, securityChoices, Modifier.weight(1f)) {
                securityChanged("smtp", it)
            }
        }
        SecurityWarning(warnings["smtp_sec"])
        Field("Username", smtpUser, errors["smtp_user"], warnings["smtp_user"]) {
            smtpUser = it
            recheck()
        }
        PasswordField(
            "Password (blank = same as IMAP)" + if (editing) "; blank keeps stored" else "",
            smtpPassword, errors["smtp_password"], warnings["smtp_password"], showSmtpPassword,
            onReveal = { showSmtpPassword = !showSmtpPassword },
        ) {
            smtpPassword = it
            recheck()
        }

        testResult?.let { r ->
            if (r.ok) {
                Text(
                    "Connection OK — IMAP and SMTP logins succeeded.",
                    color = MaterialTheme.colorScheme.primary,
                )
            } else {
                if (r.error.isNotEmpty()) {
                    Text(r.error, color = MaterialTheme.colorScheme.error)
                }
                if (r.imapError.isNotEmpty()) {
                    Text("IMAP: ${r.imapError}", color = MaterialTheme.colorScheme.error)
                } else if (r.error.isEmpty()) {
                    Text("IMAP: OK")
                }
                if (r.smtpError.isNotEmpty()) {
                    Text("SMTP: ${r.smtpError}", color = MaterialTheme.colorScheme.error)
                } else if (r.error.isEmpty()) {
                    Text("SMTP: OK")
                }
            }
        }
        (loadError ?: saveError)?.let {
            Text(it, color = MaterialTheme.colorScheme.error)
        }
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            TextButton(onClick = {
                if (dirty) confirmDiscard = true else onClose()
            }) { Text("Cancel") }
            Button(onClick = ::testConnection, enabled = loaded && !testing && !saving) {
                Text(if (testing) "Testing…" else "Test connection")
            }
            Button(onClick = ::save, enabled = loaded && !saving && !testing) {
                Text(if (saving) "Saving…" else "Save account")
            }
        }
    }

    if (confirmDiscard) {
        AlertDialog(
            onDismissRequest = { confirmDiscard = false },
            title = { Text("Discard changes?") },
            text = { Text("The account form has unsaved changes.") },
            confirmButton = {
                TextButton(onClick = {
                    confirmDiscard = false
                    onClose()
                }) { Text("Discard", color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = {
                TextButton(onClick = { confirmDiscard = false }) { Text("Keep editing") }
            },
        )
    }
}

// One setup-form connection test, parsed from the core's infallible JSON
// report (see MailNative.testAccountConnection).
private data class ConnTest(
    val ok: Boolean,
    val error: String,
    val imapOk: Boolean,
    val imapError: String,
    val smtpOk: Boolean,
    val smtpError: String,
)

private fun parseConnTest(json: String): ConnTest {
    val o = JSONObject(json)
    val imap = o.optJSONObject("imap") ?: JSONObject()
    val smtp = o.optJSONObject("smtp") ?: JSONObject()
    return ConnTest(
        ok = o.optBoolean("ok", false),
        error = o.optString("error", ""),
        imapOk = imap.optBoolean("ok", false),
        imapError = imap.optString("error", ""),
        smtpOk = smtp.optBoolean("ok", false),
        smtpError = smtp.optString("error", ""),
    )
}

@Composable
private fun Section(title: String) {    Text(
        title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
        modifier = Modifier.padding(top = 12.dp),
    )
}

@Composable
private fun Field(
    label: String,
    value: String,
    error: String?,
    warning: String?,
    keyboard: KeyboardType = KeyboardType.Text,
    modifier: Modifier = Modifier,
    onChange: (String) -> Unit,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        isError = error != null,
        supportingText = {
            (error ?: warning)?.let { Text(it) }
        },
        keyboardOptions = KeyboardOptions(keyboardType = keyboard),
        singleLine = true,
        modifier = modifier.fillMaxWidth(),
    )
}

@Composable
private fun PasswordField(
    label: String,
    value: String,
    error: String?,
    warning: String?,
    visible: Boolean,
    onReveal: () -> Unit,
    onChange: (String) -> Unit,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        isError = error != null,
        supportingText = { (error ?: warning)?.let { Text(it) } },
        visualTransformation = if (visible) VisualTransformation.None else PasswordVisualTransformation(),
        trailingIcon = {
            TextButton(onClick = onReveal) { Text(if (visible) "Hide" else "Show") }
        },
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SecurityPicker(
    label: String,
    value: String,
    choices: List<String>,
    modifier: Modifier = Modifier,
    onPick: (String) -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(
        expanded = expanded,
        onExpandedChange = { expanded = it },
        modifier = modifier,
    ) {
        OutlinedTextField(
            value = securityLabel(value),
            onValueChange = {},
            readOnly = true,
            label = { Text(label) },
            singleLine = true,
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded) },
            modifier = Modifier.fillMaxWidth().menuAnchor(
                ExposedDropdownMenuAnchorType.PrimaryNotEditable,
            ),
        )
        ExposedDropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
            for (c in choices) {
                DropdownMenuItem(
                    text = { Text(securityLabel(c)) },
                    onClick = {
                        expanded = false
                        if (c != value) onPick(c)
                    },
                )
            }
        }
    }
}

// The same words as Qt's account form for the core's security values.
private fun securityLabel(value: String): String = when (value) {
    "starttls" -> "STARTTLS"
    "none" -> "None (unencrypted)"
    else -> "SSL/TLS"
}

// The core's warning while plaintext ("none") is chosen, full width under
// the port/security row: the half-width picker is too narrow to hold it.
@Composable
private fun SecurityWarning(text: String?) {
    if (text.isNullOrEmpty()) return
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.error,
        modifier = Modifier.fillMaxWidth(),
    )
}
