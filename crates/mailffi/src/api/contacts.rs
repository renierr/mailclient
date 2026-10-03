//! Contacts, collected from sent mail and used for recipient autocomplete.

use mailcore::store::contacts;

use crate::db::shared_db;

/// Known recipients as JSON, ranked by use. An empty `prefix` lists them all
/// (the contacts manager); a non-empty one is the composer's autocomplete.
pub fn contacts_json(prefix: String) -> anyhow::Result<String> {
    let db = shared_db()?;
    let list = if prefix.trim().is_empty() {
        contacts::list(db, 200)?
    } else {
        contacts::suggest(db, &prefix, 10)?
    };
    Ok(serde_json::to_string(&list)?)
}

/// Set or clear a contact's user-defined alias. An empty alias clears it,
/// falling back to the name the mail itself carried.
pub fn set_contact_alias(address: String, alias: String) -> anyhow::Result<()> {
    let alias = alias.trim();
    let alias = (!alias.is_empty()).then_some(alias);
    Ok(contacts::set_alias(shared_db()?, address.trim(), alias)?)
}

/// Forget one auto-collected recipient.
pub fn delete_contact(address: String) -> anyhow::Result<()> {
    Ok(contacts::delete(shared_db()?, address.trim())?)
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
