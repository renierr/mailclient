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
use jni::objects::{JByteArray, JClass, JObject, JString};
use jni::refs::Global;
use jni::vm::JavaVM;
use jni::{jni_sig, jni_str, Env, EnvUnowned, JValue};
use mailcore::sync::background::{self, notify, BackgroundReport, SeenMark};
use mailcore::sync::headless;
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

/// `MailNative.backgroundPlan()`: what the host should run now, as the
/// `BackgroundPlan` JSON. Database only, no network; called again at the
/// plan's `replan_at`, when some account's quiet hours start or end.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_backgroundPlan<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let plan = background::schedule::plan(crate::db::shared_db()?);
            Ok(env.new_string(serde_json::to_string(&plan)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.plan(report, permitted, foreground, shown)`: what to do with
/// the notifications for a report, as `NotificationPlan` JSON. `shown` is a
/// JSON object of the app's notifications on screen, tag → signature.
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
            let shown: notify::Shown = serde_json::from_str(&string(env, &shown)?)?;
            let db = crate::db::shared_db()?;
            let plan = notify::plan_for(db, &report, permitted, foreground, &shown);
            Ok(env.new_string(serde_json::to_string(&plan)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.markRead(target)`: a notification's "Mark read" button, with
/// the `ReadTarget` JSON the plan gave it. Marks the cache only and returns
/// a `BackgroundReport` of what is still pending, to plan again with;
/// `pushFlags` carries the change to the server.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_markRead<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    target: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let target: notify::ReadTarget = serde_json::from_str(&string(env, &target)?)?;
            let report = notify::mark_read(crate::db::shared_db()?, &target)?;
            Ok(env.new_string(serde_json::to_string(&report)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.pushFlags(accountId)`: send the account's queued flag changes
/// over a fresh connection. Blocks for the network, so only a worker thread
/// calls it; throws when the server cannot be reached (the change stays
/// queued).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_pushFlags<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            let db = crate::db::shared_db()?;
            headless::push_flags_blocking(db, account_id)?;
            Ok(())
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

/// Drive `f` on a throwaway current-thread Tokio runtime, like
/// `headless::push_flags_blocking`: JNI worker threads have no runtime.
fn blocking<T>(f: impl std::future::Future<Output = anyhow::Result<T>>) -> Result<T> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BridgeError(format!("runtime: {e}")))?;
    rt.block_on(f).map_err(|e| BridgeError(format!("{e:#}")))
}

/// Experiment: native reader (`ReaderActivity.kt`, branch
/// `experiment/native-reader`). The activity passes ids only (never HTML —
/// bodies with inline images exceed the Binder limit) and re-reads from the
/// same database Dart uses.
///
/// `MailNative.readerMessage(folderId, uid)`: full reader payload, same JSON
/// as `api::messages::message_json` (sanitized bodies included).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let json = mailcore::feed::message_json(
                crate::db::shared_db()?,
                folder_id,
                uid.max(0) as u32,
            )?;
            Ok(env.new_string(json)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerHeaders(folderId, uid)`: `{from, to, cc, date, subject,
/// message_id, reply_to}`, same JSON as `api::messages::headers_json`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerHeaders<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let json = mailcore::feed::headers_json(
                crate::db::shared_db()?,
                folder_id,
                uid.max(0) as u32,
            )?;
            Ok(env.new_string(json)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerDocument(body, paint, paper, ink, link, quote, rule,
/// allowRemote, scale, fit)`: full document for a `WebView`, same builder as
/// `api::reader::reader_document` but with `top_space: 0` — the native
/// activity lays the header out as views above the `WebView` in one scroll,
/// so there is no overlay spacer.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerDocument<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    body: JString<'caller>,
    paint: JString<'caller>,
    paper: i32,
    ink: i32,
    link: i32,
    quote: i32,
    rule: i32,
    allow_remote: bool,
    scale: f32,
    fit: bool,
) -> JString<'caller> {
    use mailcore::html::reader::{self, Palette, Rgb};
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let rgb = |v: i32| Rgb(v as u32 & 0xFF_FFFF);
            let doc = reader::document(
                &string(env, &body)?,
                &reader::DocumentOptions {
                    paint: reader::Paint::parse(&string(env, &paint)?),
                    theme: Palette {
                        paper: rgb(paper),
                        ink: rgb(ink),
                        link: rgb(link),
                        quote: rgb(quote),
                        rule: rgb(rule),
                    },
                    allow_remote,
                    top_space: 0,
                    scale,
                    fit,
                    extra_css: "",
                },
            );
            Ok(env.new_string(doc)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Experiment: full reader actions over JNI (branch
/// `experiment/native-reader`). Same rule as everywhere else: translation
/// only — every one of these mirrors a `mailffi::api` function over the same
/// database and net thread Dart uses, so both readers queue the same jobs.

/// `MailNative.readerMessageHtml(folderId, uid, allowRemote)`: the "show
/// remote images once" path — `feed::message_html` re-sanitized with remote
/// references kept.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerMessageHtml<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
    allow_remote: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let html = mailcore::feed::message_html(
                crate::db::shared_db()?,
                folder_id,
                uid.max(0) as u32,
                allow_remote,
            )?;
            Ok(env.new_string(html)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.markReadPlan(autoMarkRead, delaySecs, unread)`: whether and
/// when opening an unread row marks it read — `{"plan","delay_secs"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_markReadPlan<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    auto_mark_read: bool,
    delay_secs: i64,
    unread: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let (plan, delay) =
                match mailcore::store::settings::mark_read_plan(auto_mark_read, delay_secs, unread)
                {
                    mailcore::store::settings::MarkReadPlan::Off => ("off", 0),
                    mailcore::store::settings::MarkReadPlan::Now => ("now", 0),
                    mailcore::store::settings::MarkReadPlan::AfterDelay(s) => ("after", s),
                };
            let json = serde_json::json!({"plan": plan, "delay_secs": delay});
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setReadFlag(accountId, folderId, uid, read)`: local flag write
/// with background push, like `api::messages::mark_read`. (Named apart from
/// the notification `markRead`, which marks a `ReadTarget`.)
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setReadFlag<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
    read: bool,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::messages::mark_read(account_id, folder_id, uid.max(0) as u32, read)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.toggleStar(accountId, folderId, uid)`: flip one message's
/// starred flag, like `api::messages::toggle_star`. The reader re-reads the
/// message for the new state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_toggleStar<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::messages::toggle_star(account_id, folder_id, uid.max(0) as u32)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

fn move_result_json<'a>(
    env: &mut Env<'a>,
    r: crate::api::mutate::MoveResult,
) -> Result<JString<'a>> {
    let json = serde_json::json!({
        "batch": r.batch,
        "label": r.label,
        "purging": r.purging,
    });
    Ok(env.new_string(serde_json::to_string(&json)?)?)
}

/// `MailNative.deleteMessage(accountId, folderId, uid)`: Trash (undoable)
/// or a purge job where Trash does not apply — `{"batch","label","purging"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::delete_messages(
                account_id,
                folder_id,
                vec![uid.max(0) as u32],
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.archiveMessage(accountId, folderId, uid)`: one-click archive.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_archiveMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::archive_messages(
                account_id,
                folder_id,
                vec![uid.max(0) as u32],
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.moveMessage(accountId, folderId, uid, destPath)`: move to any
/// folder of the same account, addressed by path.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_moveMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
    dest_path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::move_messages(
                account_id,
                folder_id,
                vec![uid.max(0) as u32],
                string(env, &dest_path)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.purgeMessage(accountId, folderId, uid)`: destroy server-side.
/// No undo — the UI confirms first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_purgeMessage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::mutate::purge_messages(account_id, folder_id, vec![uid.max(0) as u32])?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.undoMove(batch)`: take back a queued action; the status line
/// text, also when it was too late.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_undoMove<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    batch: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let text = crate::api::mutate::undo_move(string(env, &batch)?)?;
            Ok(env.new_string(text)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.undoGraceSecs()`: seconds an action stays undoable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_undoGraceSecs<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::mutate::undo_grace_secs().to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.foldersJson(accountId)`: the move picker's folder tree.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_foldersJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::folders::folders_json(account_id)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.linkInfo(url)`: a clicked link split for the examine dialog,
/// and whether it may be opened at all — `{"safe","scheme","host","path"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_linkInfo<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    url: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let i = mailcore::html::link_info(&string(env, &url)?);
            let json = serde_json::json!({
                "safe": i.safe,
                "scheme": i.scheme,
                "host": i.host,
                "path": i.path,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.cachedAttachmentBytes(attachmentId)`: cached bytes, or an
/// error when they are not downloaded yet — then `downloadMessageFiles`
/// first. Bytes cross as a `byte[]`, like the FRB `Vec<u8>`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_cachedAttachmentBytes<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    attachment_id: i64,
) -> JByteArray<'caller> {
    unowned
        .with_env(|env| -> Result<JByteArray<'caller>> {
            match crate::api::attachments::cached_attachment_bytes(attachment_id)? {
                Some(bytes) => Ok(env.byte_array_from_slice(&bytes)?),
                None => Err(BridgeError("not downloaded yet".to_string())),
            }
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.downloadMessageFiles(accountId, folderId, uid)`: fetch every
/// attachment of one message into the local cache, blocking the calling
/// worker thread (whole-message: IMAP fetches by body part within one
/// FETCH). Returns how many files landed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_downloadMessageFiles<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let db = crate::db::shared_db()?;
            let m = mailcore::store::messages::get_by_uid(db, folder_id, uid.max(0) as u32)?;
            if m.account_id != account_id {
                return Err(BridgeError(
                    "message does not belong to this account".to_string(),
                ));
            }
            let files: u64 = blocking(async {
                mailcore::sync::attachments::download(db, m.id)
                    .await
                    .map_err(anyhow::Error::msg)
            })?;
            Ok(env.new_string(files.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.writeAttachmentCopy(attachmentId, dir)`: the copy a system
/// viewer opens, under a name that cannot clash or escape `dir`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_writeAttachmentCopy<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    attachment_id: i64,
    dir: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let path =
                crate::api::attachments::write_attachment_copy(attachment_id, string(env, &dir)?)?;
            Ok(env.new_string(path)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.suggestedEmlName(folderId, uid)`: filesystem-safe `.eml` name.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_suggestedEmlName<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let name = mailcore::export::suggested_eml_name(
                crate::db::shared_db()?,
                folder_id,
                uid.max(0) as u32,
            );
            Ok(env.new_string(name)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.exportEmlBytes(folderId, uid)`: download-then-assemble as one
/// blocking call (the Dart side waits for an `Export` job event instead).
/// Fails while attachment bytes are missing and undownloadable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_exportEmlBytes<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JByteArray<'caller> {
    unowned
        .with_env(|env| -> Result<JByteArray<'caller>> {
            let uid = uid.max(0) as u32;
            blocking(async {
                mailcore::export::prepare(crate::db::shared_db()?, folder_id, uid)
                    .await
                    .map_err(anyhow::Error::msg)
            })?;
            let eml = mailcore::export::assemble_eml(crate::db::shared_db()?, folder_id, uid)?;
            Ok(env.byte_array_from_slice(&eml)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
