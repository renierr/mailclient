//! Kotlin's way into the core on Android, without a Flutter engine.
//!
//! The background check worker, the exact alarm and the IMAP IDLE push
//! service all run while the app may be closed. Booting a Flutter engine for
//! each of them costs more CPU and memory than the check itself, so they call
//! these JNI functions directly (`de.renier.mailclient.MailNative`), in the
//! same process and on the same database as the Dart side.
//!
//! Same rule as [`crate::api`]: translation only. The decisions live in
//! `mailcore` (`sync::background`, `sync::background::notify`,
//! `sync::push`); Kotlin reads Android state, posts what the plan says and
//! hands the plan back to be committed.

use std::sync::{Arc, Mutex};

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject, JString};
use jni::refs::Global;
use jni::vm::JavaVM;
use jni::{jni_sig, jni_str, Env, EnvUnowned, JValue};
use mailcore::sync::background::{self, notify, BackgroundReport, SeenMark};
use mailcore::sync::push::{PushListener, PushMonitor};
use serde::Deserialize;

/// Errors crossing into Java as a `RuntimeException`.
#[derive(Debug)]
struct BridgeError(String);

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BridgeError {}

impl From<jni::errors::Error> for BridgeError {
    fn from(e: jni::errors::Error) -> Self {
        Self(format!("jni: {e}"))
    }
}

impl From<anyhow::Error> for BridgeError {
    fn from(e: anyhow::Error) -> Self {
        Self(format!("{e:#}"))
    }
}

impl From<mailcore::StoreError> for BridgeError {
    fn from(e: mailcore::StoreError) -> Self {
        Self(e.to_string())
    }
}

impl From<serde_json::Error> for BridgeError {
    fn from(e: serde_json::Error) -> Self {
        Self(format!("json: {e}"))
    }
}

type Result<T> = std::result::Result<T, BridgeError>;

fn string(env: &Env<'_>, s: &JString<'_>) -> Result<String> {
    Ok(s.try_to_string(env)?)
}

/// `MailNative.init(dataDir)`: logging, database and vault location. Must
/// run before any other call; running it again is harmless.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_init<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    data_dir: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::startup::init_logging();
            crate::startup::use_data_dir(string(env, &data_dir)?.into())?;
            crate::db::shared_db()?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.check(trigger)`: one scheduled check (sync every inbox, then
/// report), as the `BackgroundReport` JSON. Blocks for the network run, so
/// only a worker thread calls it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_check<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    trigger: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let trigger = string(env, &trigger)?;
            let db = crate::db::shared_db()?;
            let report = background::background_check_blocking(db, &crate::db::db_path(), &trigger);
            Ok(env.new_string(serde_json::to_string(&report)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.plan(report, permitted, foreground, shown)`: what to do with
/// the notification for a report, as `NotificationPlan` JSON. `shown` is the
/// signature of the notification on screen, or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_plan<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    report: JString<'caller>,
    permitted: bool,
    foreground: bool,
    shown: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let report: BackgroundReport = serde_json::from_str(&string(env, &report)?)?;
            let shown = if shown.is_null() {
                None
            } else {
                Some(string(env, &shown)?)
            };
            let db = crate::db::shared_db()?;
            let plan = notify::plan_for(db, &report, permitted, foreground, shown.as_deref());
            Ok(env.new_string(serde_json::to_string(&plan)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// The part of a `NotificationPlan` that is committed afterwards.
#[derive(Deserialize)]
struct Settled {
    run: String,
    #[serde(default)]
    marks: Vec<SeenMark>,
    #[serde(default)]
    outcome: Option<String>,
}

/// `MailNative.commit(plan)`: the plan was carried out; record its marks as
/// seen and its outcome in the run history.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_commit<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    plan: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            let settled: Settled = serde_json::from_str(&string(env, &plan)?)?;
            let db = crate::db::shared_db()?;
            background::commit_seen(db, &settled.marks);
            if let Some(outcome) = &settled.outcome {
                background::record_outcome(db, &settled.run, outcome);
            }
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.recordOutcome(run, outcome)`: note a failed post; the marks
/// stay uncommitted so the next check reports the mail again.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_recordOutcome<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    run: JString<'caller>,
    outcome: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            let db = crate::db::shared_db()?;
            background::record_outcome(db, &string(env, &run)?, &string(env, &outcome)?);
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// The running push monitor, if any. One per process.
fn monitor() -> &'static Mutex<Option<PushMonitor>> {
    static MONITOR: Mutex<Option<PushMonitor>> = Mutex::new(None);
    &MONITOR
}

/// Forwards the monitor's callbacks to the Kotlin `PushCallbacks` object.
struct KotlinListener {
    vm: JavaVM,
    callbacks: Global<JObject<'static>>,
}

impl KotlinListener {
    fn call(&self, name: &str, run: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> Result<()>) {
        let outcome = self
            .vm
            .attach_current_thread(|env| -> Result<()> { run(env, self.callbacks.as_obj()) });
        if let Err(e) = outcome {
            log::warn!("push: {name} callback failed: {e}");
        }
    }
}

impl PushListener for KotlinListener {
    fn busy(&self, busy: bool) {
        self.call("onBusy", |env, obj| {
            env.call_method(
                obj,
                jni_str!("onBusy"),
                jni_sig!("(Z)V"),
                &[JValue::Bool(busy)],
            )?;
            Ok(())
        });
    }

    fn report(&self, report: &BackgroundReport) {
        let json = match serde_json::to_string(report) {
            Ok(json) => json,
            Err(e) => {
                log::warn!("push: report not encoded: {e}");
                return;
            }
        };
        self.call("onReport", |env, obj| {
            let json = env.new_string(&json)?;
            env.call_method(
                obj,
                jni_str!("onReport"),
                jni_sig!("(Ljava/lang/String;)V"),
                &[JValue::Object(&json)],
            )?;
            Ok(())
        });
    }
}

/// `MailNative.pushStart(callbacks, online)`: start the IDLE monitor, or
/// restart it with new callbacks.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_pushStart<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    callbacks: JObject<'caller>,
    online: bool,
) {
    unowned
        .with_env(|env| -> Result<()> {
            let listener = KotlinListener {
                vm: env.get_java_vm()?,
                callbacks: env.new_global_ref(&callbacks)?,
            };
            let mut slot = monitor().lock().unwrap_or_else(|e| e.into_inner());
            // Dropping the old monitor stops it; its accounts log out.
            *slot = None;
            *slot = Some(PushMonitor::start(
                crate::db::db_path(),
                Arc::new(listener),
                online,
            )?);
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

fn with_monitor(f: impl FnOnce(&PushMonitor)) {
    if let Some(m) = monitor().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        f(m);
    }
}

/// `MailNative.pushKeepalive()`: from the keep-alive alarm.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_pushKeepalive<'caller>(
    _unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) {
    with_monitor(PushMonitor::keepalive);
}

/// `MailNative.pushNetwork(online)`: the default network changed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_pushNetwork<'caller>(
    _unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    online: bool,
) {
    with_monitor(|m| m.network_changed(online));
}

/// `MailNative.pushStop()`: stop the monitor; returns at once.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_pushStop<'caller>(
    _unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) {
    monitor().lock().unwrap_or_else(|e| e.into_inner()).take();
}
