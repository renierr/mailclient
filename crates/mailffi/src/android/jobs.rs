//! The job-event listener Kotlin subscribes to, and busy-state reads.

use super::*;

use std::sync::{Arc, Mutex};

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject, JString};
use jni::refs::Global;
use jni::vm::JavaVM;
use jni::{jni_sig, jni_str, EnvUnowned, JValue};

/// Step 0b sync jobs (`android/PLAN.md`): queue onto `mailclient-net` like
/// the FRB calls, and hear back through a listener instead of the Dart
/// stream. Registering replaces the previous listener (same hot-restart
/// rule as `job_events`); every event also crosses as one JSON string, the
/// same convention as every other JNI read.
///
/// The Kotlin listener object the net thread calls back. Behind an `Arc` so
/// a callback runs without the slot locked: Kotlin may queue a job from
/// inside `onJobEvent`, and that queue call forwards a busy event itself.
type JobListener = Arc<(JavaVM, Global<JObject<'static>>)>;

fn job_listener() -> &'static Mutex<Option<JobListener>> {
    static LISTENER: Mutex<Option<JobListener>> = Mutex::new(None);
    &LISTENER
}

/// `MailNative.setJobListener(callbacks)`: subscribe `onJobEvent(json)` for
/// the life of the process.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setJobListener<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    callbacks: JObject<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            let listener = Arc::new((env.get_java_vm()?, env.new_global_ref(&callbacks)?));
            *job_listener().lock().unwrap_or_else(|e| e.into_inner()) = Some(listener);
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Hand one event's JSON to the Kotlin listener, if registered. The VM
/// attach is per call, like the push monitor's.
fn deliver_job_json(json: &str) {
    let listener = job_listener()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Some(listener) = listener else {
        return;
    };
    let (vm, callbacks) = &*listener;
    let outcome = vm.attach_current_thread(|env| -> Result<()> {
        let json = env.new_string(json)?;
        env.call_method(
            callbacks.as_obj(),
            jni_str!("onJobEvent"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&json)],
        )?;
        Ok(())
    });
    if let Err(e) = outcome {
        log::warn!("jobs: onJobEvent callback failed: {e}");
    }
}

/// Forward one finished (or progress) job event to Kotlin. Called from
/// [`crate::api::events::emit_event`], i.e. on the net thread, after a
/// finished job has left the in-flight table — so `busy` already says
/// whether anything else is still queued.
pub(crate) fn forward_job_event(event: &crate::api::events::JobEvent, key: &str) {
    use crate::api::events::JobPhase;
    let json = serde_json::json!({
        "kind": event.kind,
        "phase": match event.phase {
            JobPhase::Progress => "progress",
            JobPhase::Finished => "finished",
        },
        "status": event.status,
        "ok": event.ok,
        "outcome": event.outcome,
        "account_id": event.account_id,
        "folder_id": event.folder_id,
        "key": key,
        "busy": crate::net::busy_snapshot(),
    })
    .to_string();
    deliver_job_json(&json);
}

/// `MailNative.attachmentsJobKey(folderId, uid)`: the key a message's
/// download job reports under, so a waiter can register for exactly that job
/// before queuing it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_attachmentsJobKey<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::net::attachments_key(folder_id, uid_arg(uid)?))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.createFolderJobKey(accountId)`: the key a folder creation
/// reports under (see `attachmentsJobKey`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_createFolderJobKey<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::net::create_folder_key(account_id))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Tell Kotlin a job was queued (`phase: "queued"`), with the in-flight
/// table as it stood right after the insert. Called on the thread that
/// queued it.
pub(crate) fn forward_busy(kind: &str, busy: &crate::net::BusySnapshot) {
    let json = serde_json::json!({
        "kind": kind,
        "phase": "queued",
        "busy": busy,
    })
    .to_string();
    deliver_job_json(&json);
}

/// `MailNative.netBusy()`: the in-flight table as JSON
/// (`{generation, kinds, keys}`), for a screen that starts while jobs are
/// already running, and for diagnostics: a key listed here was queued and
/// has not reported back yet.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_netBusy<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let json = serde_json::to_string(&crate::net::busy_snapshot())?;
            Ok(env.new_string(json)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
