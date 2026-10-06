package de.renier.mailclient.ui.contacts

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import de.renier.mailclient.MailNative
import de.renier.mailclient.R
import de.renier.mailclient.ui.common.FormDialog
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/** One auto-collected contact (`mailcore::store::contacts::Contact`). */
private data class Contact(
    val address: String,
    val name: String,
    val alias: String,
    val timesSeen: Long,
    val sentCount: Long,
) {
    val shown: String get() = alias.ifEmpty { name }

    companion object {
        fun of(o: JSONObject) = Contact(
            address = o.optString("address"),
            name = o.optString("name").takeIf { !o.isNull("name") }.orEmpty(),
            alias = o.optString("alias").takeIf { !o.isNull("alias") }.orEmpty(),
            timesSeen = o.optLong("times_seen"),
            sentCount = o.optLong("sent_count"),
        )
    }
}

/** A cleanup suggestion: the contact and why (`automated`, `stale`). */
private data class Candidate(val contact: Contact, val reasons: List<String>) {
    // The machine reasons in the Qt and Flutter wording.
    val reasonText: String
        get() = reasons.joinToString("; ") {
            when (it) {
                "automated" -> "looks like an automated sender"
                "stale" -> "seen only once, long ago"
                else -> it
            }
        }
}

private fun parseContacts(json: String): List<Contact> {
    val arr = JSONArray(json)
    return (0 until arr.length()).mapNotNull { arr.optJSONObject(it)?.let(Contact::of) }
}

private fun parseCandidates(json: String): List<Candidate> {
    val arr = JSONArray(json)
    return (0 until arr.length()).mapNotNull { i ->
        val o = arr.optJSONObject(i) ?: return@mapNotNull null
        val reasons = o.optJSONArray("reasons")
        Candidate(
            contact = Contact.of(o.optJSONObject("contact") ?: return@mapNotNull null),
            reasons = (0 until (reasons?.length() ?: 0)).map { reasons!!.optString(it) },
        )
    }
}

/**
 * The contacts manager: recipients collected from mail, searchable by
 * alias, name or address prefix; an alias renames someone just for you
 * (and in the composer's suggestions); remove forgets an address until
 * mail from it arrives again. Review suggestions lists automated senders
 * and long-unseen one-offs for a confirmed bulk removal.
 */
@Composable
fun ContactsScreen() {
    val scope = rememberCoroutineScope()
    var query by remember { mutableStateOf("") }
    var contacts by remember { mutableStateOf<List<Contact>?>(null) }
    var reviewing by remember { mutableStateOf(false) }
    var candidates by remember { mutableStateOf<List<Candidate>?>(null) }
    var selected by remember { mutableStateOf(setOf<String>()) }
    var error by remember { mutableStateOf<String?>(null) }
    var reload by remember { mutableIntStateOf(0) }
    var editing by remember { mutableStateOf<Contact?>(null) }
    var removing by remember { mutableStateOf<Contact?>(null) }
    var removingSelected by remember { mutableStateOf(false) }

    LaunchedEffect(query, reload, reviewing) {
        if (reviewing) {
            withContext(Dispatchers.IO) { runCatching { parseCandidates(MailNative.cleanupCandidatesJson()) } }
                .onSuccess { list ->
                    candidates = list
                    selected = selected.intersect(list.map { it.contact.address }.toSet())
                }
                .onFailure { error = it.message }
        } else {
            // Typing settles before the query runs.
            if (query.isNotEmpty()) delay(250)
            withContext(Dispatchers.IO) { runCatching { parseContacts(MailNative.contactsJson(query.trim())) } }
                .onSuccess { contacts = it }
                .onFailure { error = it.message }
        }
    }

    // Run a write, then re-read; its failure stays on this page.
    fun write(work: () -> Unit) {
        scope.launch {
            withContext(Dispatchers.IO) { runCatching(work) }
                .onSuccess { error = null }
                .onFailure { error = it.message ?: "Failed" }
            reload++
        }
    }

    Column(modifier = Modifier.fillMaxSize()) {
        Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
            if (!reviewing) {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    singleLine = true,
                    label = { Text("Search by alias, name or address") },
                    leadingIcon = { Icon(painterResource(R.drawable.ic_search), null) },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            Text(
                if (reviewing) {
                    "These look like automated senders or addresses seen only once, long ago. Tick the ones to forget."
                } else {
                    "Auto-collected from transferred mail. Set an alias to rename someone just for you."
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 8.dp),
            )
            error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 4.dp)) }
            ReviewBar(
                reviewing = reviewing,
                candidates = candidates.orEmpty(),
                selected = selected,
                onEnter = {
                    reviewing = true
                    candidates = null
                    selected = emptySet()
                },
                onExit = {
                    reviewing = false
                    selected = emptySet()
                },
                onSelectAll = { all -> selected = if (all) emptySet() else candidates.orEmpty().map { it.contact.address }.toSet() },
                onRemoveSelected = { removingSelected = true },
            )
        }
        HorizontalDivider()
        if (reviewing) {
            val list = candidates
            when {
                list == null -> Centered { CircularProgressIndicator() }
                list.isEmpty() -> Centered { Muted("No cleanup suggestions — your list looks tidy") }
                else -> LazyColumn(modifier = Modifier.fillMaxSize()) {
                    items(list, key = { it.contact.address }) { c ->
                        CandidateRow(c, checked = c.contact.address in selected) { on ->
                            selected = if (on) selected + c.contact.address else selected - c.contact.address
                        }
                        HorizontalDivider()
                    }
                }
            }
        } else {
            val list = contacts
            when {
                list == null -> Centered { CircularProgressIndicator() }
                list.isEmpty() -> Centered { Muted(if (query.isEmpty()) "No contacts yet" else "No matching contacts found") }
                else -> LazyColumn(modifier = Modifier.fillMaxSize()) {
                    items(list, key = { it.address }) { c ->
                        ContactRow(c, onEdit = { editing = c }, onRemove = { removing = c })
                        HorizontalDivider()
                    }
                }
            }
        }
    }

    editing?.let { c ->
        AliasDialog(
            contact = c,
            onSave = { alias ->
                editing = null
                write { MailNative.setContactAlias(c.address, alias) }
            },
            onDismiss = { editing = null },
        )
    }
    removing?.let { c ->
        ConfirmRemove(
            title = "Remove contact?",
            text = "Forget ${c.address}? It reappears the next time mail arrives from it.",
            onConfirm = {
                removing = null
                write { MailNative.deleteContact(c.address) }
            },
            onDismiss = { removing = null },
        )
    }
    if (removingSelected) {
        val n = selected.size
        ConfirmRemove(
            title = "Remove contacts?",
            text = "Forget $n ${if (n == 1) "contact" else "contacts"}? " +
                "${if (n == 1) "It reappears" else "They reappear"} the next time mail arrives from " +
                "${if (n == 1) "it" else "them"}.",
            onConfirm = {
                removingSelected = false
                val addresses = JSONArray(selected.toList()).toString()
                selected = emptySet()
                write { MailNative.deleteContacts(addresses) }
            },
            onDismiss = { removingSelected = false },
        )
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ReviewBar(
    reviewing: Boolean,
    candidates: List<Candidate>,
    selected: Set<String>,
    onEnter: () -> Unit,
    onExit: () -> Unit,
    onSelectAll: (Boolean) -> Unit,
    onRemoveSelected: () -> Unit,
) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
        modifier = Modifier.padding(top = 8.dp),
    ) {
        if (!reviewing) {
            OutlinedButton(onClick = onEnter) { Text("Review suggestions") }
            return@FlowRow
        }
        TextButton(onClick = onExit) { Text("Back") }
        if (candidates.isNotEmpty()) {
            val all = selected.size == candidates.size
            TextButton(onClick = { onSelectAll(all) }) { Text(if (all) "Clear" else "Select all") }
        }
        Button(
            onClick = onRemoveSelected,
            enabled = selected.isNotEmpty(),
            colors = ButtonDefaults.buttonColors(
                containerColor = MaterialTheme.colorScheme.error,
                contentColor = MaterialTheme.colorScheme.onError,
            ),
        ) { Text("Remove selected (${selected.size})") }
    }
}

@Composable
private fun ContactRow(c: Contact, onEdit: () -> Unit, onRemove: () -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth().clickable(onClick = onEdit).padding(start = 16.dp, top = 8.dp, bottom = 8.dp),
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Name(c)
            Text(c.address, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                listOfNotNull(
                    "seen ${c.timesSeen}",
                    if (c.sentCount > 0) "sent ${c.sentCount}" else null,
                    if (c.alias.isNotEmpty() && c.name.isNotEmpty() && c.alias != c.name) "Was: ${c.name}" else null,
                ).joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        IconButton(onClick = onEdit) { Icon(painterResource(R.drawable.ic_edit), "Edit alias") }
        IconButton(onClick = onRemove) { Icon(painterResource(R.drawable.ic_delete), "Remove") }
    }
}

@Composable
private fun CandidateRow(c: Candidate, checked: Boolean, onCheck: (Boolean) -> Unit) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier.fillMaxWidth().clickable { onCheck(!checked) }.padding(end = 16.dp, top = 4.dp, bottom = 4.dp),
    ) {
        Checkbox(checked = checked, onCheckedChange = onCheck)
        Column(modifier = Modifier.weight(1f)) {
            Name(c.contact)
            Text(
                listOfNotNull(
                    c.contact.address,
                    c.reasonText.ifEmpty { null },
                    if (c.contact.sentCount > 0) "sent ${c.contact.sentCount}" else null,
                ).joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun Name(c: Contact) {
    Text(
        c.shown.ifEmpty { "(no alias)" },
        style = MaterialTheme.typography.titleSmall,
        fontStyle = if (c.shown.isEmpty()) FontStyle.Italic else null,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
    )
}

/** Alias for one address; empty clears it back to the name mail carried. */
@Composable
private fun AliasDialog(contact: Contact, onSave: (String) -> Unit, onDismiss: () -> Unit) {
    var alias by remember { mutableStateOf(contact.alias) }
    FormDialog(title = "Alias", confirmLabel = "Save", onConfirm = { onSave(alias.trim()) }, onDismiss = onDismiss) {
        Text(contact.address, style = MaterialTheme.typography.bodyMedium)
        OutlinedTextField(
            value = alias,
            onValueChange = { alias = it },
            singleLine = true,
            placeholder = { Text(contact.name.ifEmpty { "Alias name" }) },
            supportingText = { Text("Leave empty to use the name the mail carries.") },
            modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
        )
    }
}

@Composable
private fun ConfirmRemove(title: String, text: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = {
            Button(
                onClick = onConfirm,
                colors = ButtonDefaults.buttonColors(
                    containerColor = MaterialTheme.colorScheme.error,
                    contentColor = MaterialTheme.colorScheme.onError,
                ),
            ) { Text("Remove") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun Centered(content: @Composable () -> Unit) {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { content() }
}

@Composable
private fun Muted(text: String) {
    Text(text, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(24.dp))
}
