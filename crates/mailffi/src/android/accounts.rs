//! Account CRUD, the setup form and the account switch (step 0a).

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;

/// Step 0a shell reads (`android/PLAN.md`): accounts, folder navigation and
/// the outbox pill. Thin wraps of `mailffi::api` — JSON or plain strings
/// across, ids back as strings.
///
/// `MailNative.accountsJson()`: every account, for the switcher.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_accountsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::accounts::accounts_json()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.accountForm(id)`: one account as edit-form JSON. Never carries
/// a password — blank stays "keep the stored one".
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_accountForm<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::accounts::account_form(id)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.accountFormDefaults()`: a new form's starting values and
/// security choices.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_accountFormDefaults<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::accounts::account_form_defaults())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.accountGuess(email)`: server guesses for a typed address.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_accountGuess<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    email: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::accounts::account_guess(string(env, &email)?))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.accountPortForSecurity(protocol, oldSec, newSec, port)`: the
/// port field after a security change.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_accountPortForSecurity<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    protocol: JString<'caller>,
    old_sec: JString<'caller>,
    new_sec: JString<'caller>,
    port: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(
                env.new_string(crate::api::accounts::account_port_for_security(
                    string(env, &protocol)?,
                    string(env, &old_sec)?,
                    string(env, &new_sec)?,
                    string(env, &port)?,
                ))?,
            )
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.accountFormCheck(form, editing)`: per-field errors and
/// warnings, the same check saving runs.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_accountFormCheck<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    form: JString<'caller>,
    editing: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::accounts::account_form_check(
                string(env, &form)?,
                editing,
            ))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.saveAccount(form)`: create or update an account; the id back
/// as a string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_saveAccount<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    form: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let id = crate::api::accounts::save_account(string(env, &form)?)?;
            Ok(env.new_string(id.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.testAccountConnection(form)`: live IMAP + SMTP login check
/// for the setup form; the infallible JSON report (see
/// `api::accounts::test_account_connection`). Blocking (up to the probe
/// timeouts): call off the UI thread (`Dispatchers.IO`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_testAccountConnection<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    form: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let form = string(env, &form)?;
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow::anyhow!("cannot start network: {e}"))?;
            let report = rt.block_on(crate::api::accounts::test_account_connection(form));
            Ok(env.new_string(report)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.deleteAccount(id)`: delete with folders, messages and secrets;
/// the account to show instead back as a string (`-1`: none left).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteAccount<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let next = crate::api::accounts::delete_account(id)?;
            Ok(env.new_string(next.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.initialSelection()`: where the app opens —
/// `{"account_id","folder_id"}`, `-1` when there is nothing.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_initialSelection<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let s = crate::api::accounts::initial_selection()?;
            let json = serde_json::json!({
                "account_id": s.account_id,
                "folder_id": s.folder_id,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.selectAccount(id)`: switch the active account, landing folder
/// back as `{"account_id","folder_id"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_selectAccount<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let s = crate::api::accounts::select_account(id)?;
            let json = serde_json::json!({
                "account_id": s.account_id,
                "folder_id": s.folder_id,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
