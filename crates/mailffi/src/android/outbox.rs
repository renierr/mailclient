//! The outbox screen, scheduled sends and their alarms.

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;

/// `MailNative.outboxStatusJson(accountId)`: the status pill's counts —
/// `{queued, sending, failed, retryable, pending}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_outboxStatusJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::outbox::outbox_status_json(account_id)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.outboxJson(accountId)`: the account's unsent mail with states
/// and errors, for the outbox screen.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_outboxJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::outbox::outbox_json(account_id)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.dismissOutbox(accountId, id)`: forget one queued send.
/// Local-only, never resends.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_dismissOutbox<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::outbox::dismiss_outbox(account_id, id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.rescheduleOutbox(accountId, id, sendAt)`: move a held row to a
/// new future instant. Returns the normalized UTC instant. Local-only.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_rescheduleOutbox<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    id: i64,
    send_at: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let at = crate::api::outbox::reschedule_outbox(account_id, id, string(env, &send_at)?)?;
            Ok(env.new_string(at)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.sendNowOutbox(accountId, id)`: send a waiting scheduled row
/// now instead of at its time (due at once for the alarm). Local-only.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_sendNowOutbox<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::outbox::send_now_outbox(account_id, id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.sendDueInMs()`: milliseconds until the earliest waiting
/// scheduled send across all accounts wants a flush (`-1` = none, `0` = due
/// now; `mailcore::outbox::due_in_ms`). What the exact alarm arms from.
/// Local read.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_sendDueInMs<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> i64 {
    unowned
        .with_env(|_env| -> Result<i64> {
            Ok(mailcore::outbox::due_in_ms(crate::db::shared_db()?))
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.outboxEdit(accountId, id, dir)`: take an unsent mail out of
/// the outbox into the composer (`mailcore::outbox::take_for_edit`), its
/// files staged under `dir`. Returns the `OutboxEdit` as JSON; the row is
/// gone once this returns. Local-only.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_outboxEdit<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    id: i64,
    dir: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let dir = string(env, &dir)?;
            let edit = mailcore::outbox::take_for_edit(
                crate::db::shared_db()?,
                account_id,
                id,
                std::path::Path::new(&dir),
            )?;
            Ok(env.new_string(serde_json::to_string(&edit)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.takeSendFailures()`: scheduled sends that gave up and were
/// not reported yet, as a JSON array of `FailedSend`
/// (`mailcore::store::queue::take_failure_notices`); each is reported once.
/// For the "not sent" notification after a background run. Local.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_takeSendFailures<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let failed = mailcore::store::queue::take_failure_notices(crate::db::shared_db()?)?;
            Ok(env.new_string(serde_json::to_string(&failed)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.sendAtError(sendAt)`: why `sendAt` (any RFC3339 instant)
/// cannot be a scheduled send's time, `""` when it can
/// (`mailcore::outbox::send_at_error`). The Send-later dialog's check.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_sendAtError<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    send_at: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let error = mailcore::outbox::send_at_error(&string(env, &send_at)?);
            Ok(env.new_string(error.unwrap_or_default())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.sendDue()`: flush every due scheduled send, whatever
/// account holds it (no poll gating). Blocks for the network run, so only
/// a worker thread calls it. Returns `{sent, skipped, errors, run}` as
/// JSON; outbox-only, so no inbox sync and no new-mail notification.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_sendDue<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::outbox::send_due()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
