//! Contacts, collected from sent mail and used for recipient autocomplete.

use mailcore::store::contacts;

use crate::db::shared_db;

/// Known recipients as JSON, ranked by use. An empty `prefix` lists them all
/// (the contacts manager); a non-empty one is the composer's autocomplete.
pub fn contacts_json(prefix: String) -> anyhow::Result<String> {
    Ok(contacts::contacts_json(shared_db()?, &prefix)?)
}

/// Set or clear a contact's user-defined alias. An empty alias clears it,
/// falling back to the name the mail itself carried.
pub fn set_contact_alias(address: String, alias: String) -> anyhow::Result<()> {
    let alias = alias.trim();
    let alias = (!alias.is_empty()).then_some(alias);
    Ok(contacts::set_alias(shared_db()?, address.trim(), alias)?)
}

/// Replace the phone's own contact list snapshot (Android passes what it
/// read from ContactsContract behind `READ_CONTACTS`, as
/// `[{"name": …, "address": …}]`). Merged into the recipient field's
/// suggestions, phone matches ranked first. Returns how many entries were
/// kept.
pub fn set_phone_contacts(json: String) -> anyhow::Result<u32> {
    let entries: Vec<contacts::PhoneEntry> = serde_json::from_str(&json)?;
    Ok(contacts::set_phone_contacts(&entries) as u32)
}

/// Drop the snapshot: the setting behind it was switched off, so the phone
/// book is not held in memory any longer.
pub fn clear_phone_contacts() {
    contacts::clear_phone_contacts();
}

/// Composer recipient suggestions as JSON: the mail-collected contacts with
/// the phone's own list merged in (`set_phone_contacts`).
pub fn recipient_json(prefix: String) -> anyhow::Result<String> {
    Ok(contacts::recipient_json(shared_db()?, &prefix)?)
}

/// Forget one auto-collected recipient.
pub fn delete_contact(address: String) -> anyhow::Result<()> {
    Ok(contacts::delete(shared_db()?, address.trim())?)
}

/// Forget several auto-collected recipients at once (the cleanup review's
/// multi-select). Returns how many were removed.
pub fn delete_contacts(addresses: Vec<String>) -> anyhow::Result<u64> {
    let refs: Vec<&str> = addresses.iter().map(String::as_str).collect();
    Ok(contacts::delete_many(shared_db()?, &refs)?)
}

/// Contacts the cleanup review suggests removing (automated senders,
/// long-unseen one-offs) as JSON, each with machine-readable `reasons`.
pub fn cleanup_candidates_json() -> anyhow::Result<String> {
    let cands = contacts::cleanup_candidates(shared_db()?, 200)?;
    Ok(serde_json::to_string(&cands)?)
}

/// The recipient address currently being typed: the last `,`/`;` segment
/// outside double quotes (`compose::recipient_segment`), trimmed.
#[flutter_rust_bridge::frb(sync)]
pub fn recipient_segment(text: String) -> String {
    mailcore::compose::recipient_segment(&text).to_string()
}

/// The field after completing its current segment with `replacement`
/// (`compose::replace_recipient_segment`).
#[flutter_rust_bridge::frb(sync)]
pub fn replace_recipient_segment(text: String, replacement: String) -> String {
    mailcore::compose::replace_recipient_segment(&text, &replacement)
}
