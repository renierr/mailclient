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

/// `MailNative.folderIdForPath(accountId, path)`: path to local id, back as
/// a string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_folderIdForPath<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let id = crate::api::folders::folder_id_for_path(account_id, string(env, &path)?)?;
            Ok(env.new_string(id.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.folderPath(folderId)`: the IMAP path of a folder.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_folderPath<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::folders::folder_path(folder_id)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setFolderSubscribed(folderId, subscribed)`: show or hide a
/// folder in the sidebar (display-only, cache kept).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setFolderSubscribed<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    subscribed: bool,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::folders::set_folder_subscribed(folder_id, subscribed)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.folderCounts(folderId)`: local vs server totals plus the "Show
/// older" state — `{"cached","server","older","can_load_older"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_folderCounts<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let c = crate::api::folders::folder_counts(folder_id)?;
            let older = match c.older {
                crate::api::folders::OlderState::Unchecked => "unchecked",
                crate::api::folders::OlderState::Partial => "partial",
                crate::api::folders::OlderState::Empty => "empty",
                crate::api::folders::OlderState::Complete => "complete",
            };
            let json = serde_json::json!({
                "cached": c.cached,
                "server": c.server,
                "older": older,
                "can_load_older": c.can_load_older,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

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

/// Step 0b sync jobs (`android/PLAN.md`): queue onto `mailclient-net` like
/// the FRB calls, and hear back through a listener instead of the Dart
/// stream. Registering replaces the previous listener (same hot-restart
/// rule as `job_events`); every event also crosses as one JSON string, the
/// same convention as every other JNI read.
///
/// The Kotlin listener object the net thread calls back.
fn job_listener() -> &'static Mutex<Option<(JavaVM, Global<JObject<'static>>)>> {
    static LISTENER: Mutex<Option<(JavaVM, Global<JObject<'static>>)>> = Mutex::new(None);
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
            let mut slot = job_listener().lock().unwrap_or_else(|e| e.into_inner());
            *slot = Some((env.get_java_vm()?, env.new_global_ref(&callbacks)?));
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.clearJobListener()`: stop delivering job events to Kotlin.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_clearJobListener<'caller>(
    _unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) {
    job_listener()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
}

/// Forward one finished (or progress) job event to the Kotlin listener, if
/// registered. Called from [`crate::api::events::emit_event`], i.e. on the
/// net thread — the VM attach is per call, like the push monitor's.
pub(crate) fn forward_job_event(event: &crate::api::events::JobEvent) {
    use crate::api::events::JobPhase;
    let guard = job_listener().lock().unwrap_or_else(|e| e.into_inner());
    let Some((vm, callbacks)) = guard.as_ref() else {
        return;
    };
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
    })
    .to_string();
    let outcome = vm.attach_current_thread(|env| -> Result<()> {
        let json = env.new_string(&json)?;
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

/// `MailNative.syncAccount(accountId)`: queue a full account sync; returns
/// at once, the result arrives as a `Sync` finished event. Throws when a
/// sync for the account is already queued (`spawn` dedupe).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_syncAccount<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::sync_account(account_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.syncFolder(accountId, folderId)`: fill one opened folder's
/// newest window, after the cached rows have painted.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_syncFolder<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::sync_folder(account_id, folder_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.loadOlderMessages(accountId, folderId)`: the next older batch,
/// for the list's "Show older" row.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_loadOlderMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::load_older_messages(account_id, folder_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.refreshFolders(accountId)`: LIST only — new/renamed/deleted
/// folders appear. A `Folders` finished event says when.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_refreshFolders<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::refresh_folders(account_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.refreshServerCapabilities(accountId)`: the About view's
/// capability list, as the finishing event's JSON status payload.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_refreshServerCapabilities<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::refresh_server_capabilities(account_id)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.backgroundMarkSeen()`: the inbox cache counts as seen while
/// the app is open — no network, answers inline.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_backgroundMarkSeen<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::sync::background_mark_seen()?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.backgroundRunHistory()`: recent background ticks for the
/// Settings diagnostics.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_backgroundRunHistory<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::sync::background_run_history()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Step 0c list reads + bulk mutate (`android/PLAN.md`): one page of rows
/// plus the selection-shaped flag writes, moves and purges — single-folder
/// (`Vec<u32>`) and cross-folder search-hit flavours, like `mailffi::api`.
///
/// Selections cross as JSON (`[1,2,3]`, `[{"folder":"INBOX","uid":1}]`), the
/// same convention as everything else here — no JNI array plumbing.
fn uids_json(raw: &str) -> Result<Vec<u32>> {
    let v: serde_json::Value = serde_json::from_str(raw)?;
    let arr = v
        .as_array()
        .ok_or_else(|| BridgeError("uids must be a JSON array".to_string()))?;
    arr.iter()
        .map(|u| {
            u.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| BridgeError(format!("bad uid in {raw}")))
        })
        .collect()
}

fn hits_json(raw: &str) -> Result<Vec<crate::api::mutate::Hit>> {
    let v: serde_json::Value = serde_json::from_str(raw)?;
    let arr = v
        .as_array()
        .ok_or_else(|| BridgeError("hits must be a JSON array".to_string()))?;
    arr.iter()
        .map(|h| {
            let folder = h
                .get("folder")
                .and_then(|f| f.as_str())
                .ok_or_else(|| BridgeError(format!("hit without folder in {raw}")))?
                .to_string();
            let uid = h
                .get("uid")
                .and_then(|u| u.as_u64())
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| BridgeError(format!("hit without uid in {raw}")))?;
            Ok(crate::api::mutate::Hit { folder, uid })
        })
        .collect()
}

/// `MailNative.messagesJson(folderId, limit, offset)`: one list page in the
/// persisted sort order — `[{uid, subject, from, date, date_key, snippet,
/// unread, starred, has_attachments}]`, no bodies.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_messagesJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    limit: i64,
    offset: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::messages::messages_json(
                folder_id, limit, offset,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.markReadMany(accountId, folderId, uids, read)`: local flag
/// write + background push; how many rows changed, back as a string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_markReadMany<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
    read: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = crate::api::messages::mark_read_many(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
                read,
            )?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setStarMany(accountId, folderId, uids, starred)`: same for
/// the starred flag.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setStarMany<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
    starred: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = crate::api::messages::set_star_many(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
                starred,
            )?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.markReadHits(accountId, hits, read)`: the read flag over
/// cross-folder search hits.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_markReadHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
    read: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = crate::api::messages::mark_read_hits(
                account_id,
                hits_json(&string(env, &hits)?)?,
                read,
            )?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setStarHits(accountId, hits, starred)`: the starred flag over
/// cross-folder search hits.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setStarHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
    starred: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = crate::api::messages::set_star_hits(
                account_id,
                hits_json(&string(env, &hits)?)?,
                starred,
            )?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.deleteMessages(accountId, folderId, uids)`: Trash (undoable)
/// or purge where Trash does not apply — `{"batch","label","purging"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::delete_messages(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.archiveMessages(accountId, folderId, uids)`: one-click
/// archive of a selection.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_archiveMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::archive_messages(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.moveMessages(accountId, folderId, uids, destPath)`: a
/// selection to any folder of the same account, addressed by path.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_moveMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
    dest_path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::move_messages(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
                string(env, &dest_path)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.purgeMessages(accountId, folderId, uids)`: destroy
/// server-side. No undo — the UI confirms first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_purgeMessages<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uids: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::mutate::purge_messages(
                account_id,
                folder_id,
                uids_json(&string(env, &uids)?)?,
            )?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.deleteHits(accountId, hits)`: search hits across folders — one
/// Undo for what goes to Trash, one purge job for the shares that destroy
/// (the UI confirmed those first).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::delete_hits(account_id, hits_json(&string(env, &hits)?)?)?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.archiveHits(accountId, hits)`: hits across folders, one Undo.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_archiveHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::archive_hits(account_id, hits_json(&string(env, &hits)?)?)?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.moveHits(accountId, hits, destPath)`: hits across folders to
/// `destPath`, one Undo.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_moveHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
    dest_path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::mutate::move_hits(
                account_id,
                hits_json(&string(env, &hits)?)?,
                string(env, &dest_path)?,
            )?;
            move_result_json(env, r)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.purgeHits(accountId, hits)`: destroy hits across folders in
/// one job. No undo — the UI confirms first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_purgeHits<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    hits: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::mutate::purge_hits(account_id, hits_json(&string(env, &hits)?)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.createFolder(accountId, path)`: queued IMAP folder creation;
/// a `Folders` finished event says when. `/` separates levels, missing
/// parents are created, an existing path is success.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_createFolder<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    path: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::mutate::create_folder(account_id, string(env, &path)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
