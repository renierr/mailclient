//! Contacts, autocomplete and the recipient segment helpers.

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::sys::jint;
use jni::EnvUnowned;

/// `MailNative.contactsJson(prefix)`: contacts matching an alias, name or
/// address prefix — `""` lists the manager's 200, a typed prefix the
/// autocomplete's 10.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_contactsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    prefix: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::contacts::contacts_json(string(env, &prefix)?)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setPhoneContacts(json)`: hand the phone's own contact list
/// to the core (`[{"name", "address"}]`), which merges it into the
/// recipient field's suggestions with phone matches ranked first. Returns
/// how many of the entries were kept.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setPhoneContacts<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    json: JString<'caller>,
) -> jint {
    unowned
        .with_env(|env| -> Result<jint> {
            Ok(crate::api::contacts::set_phone_contacts(string(env, &json)?)? as jint)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.clearPhoneContacts()`: forget the snapshot the core holds,
/// called when the setting behind it is switched off.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_clearPhoneContacts<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::contacts::clear_phone_contacts();
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.recipientJson(segment)`: the composer's recipient
/// suggestions — `contactsJson` merged with the phone's own list, so a
/// saved person outranks an address mail merely carried.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_recipientJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    segment: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::contacts::recipient_json(string(
                env, &segment,
            )?)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setContactAlias(address, alias)`: an empty alias clears it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setContactAlias<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    address: JString<'caller>,
    alias: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::contacts::set_contact_alias(string(env, &address)?, string(env, &alias)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.deleteContact(address)`: remove one contact (it reappears on
/// the next mail from them).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteContact<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    address: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::contacts::delete_contact(string(env, &address)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

fn strings_json(raw: &str) -> Result<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(raw)?;
    v.as_array()
        .ok_or_else(|| BridgeError("expected a JSON string array".to_string()))?
        .iter()
        .map(|s| {
            s.as_str()
                .map(str::to_string)
                .ok_or_else(|| BridgeError(format!("bad string in {raw}")))
        })
        .collect()
}

/// `MailNative.deleteContacts(addresses)`: bulk remove from a JSON string
/// array — how many went, back as a string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteContacts<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    addresses: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n =
                crate::api::contacts::delete_contacts(strings_json(&string(env, &addresses)?)?)?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.cleanupCandidatesJson()`: automated senders and long-unseen
/// one-offs with machine reasons, for the cleanup review.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_cleanupCandidatesJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::contacts::cleanup_candidates_json()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.recipientSegment(text)`: the autocomplete's current segment —
/// the last `,`/`;` piece outside quotes.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_recipientSegment<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    text: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::contacts::recipient_segment(string(env, &text)?))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.replaceRecipientSegment(text, replacement)`: swap the current
/// segment for the picked address.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_replaceRecipientSegment<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    text: JString<'caller>,
    replacement: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(
                env.new_string(crate::api::contacts::replace_recipient_segment(
                    string(env, &text)?,
                    string(env, &replacement)?,
                ))?,
            )
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
