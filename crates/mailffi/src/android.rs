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

/// Step 0d composer/send (`android/PLAN.md`): thin wraps of
/// `mailffi::api::composer`. Sends and draft saves queue onto
/// `mailclient-net` (progress/finished events on the 0b listener);
/// validation runs inline, so a bad form throws with the composer open.
///
/// `MailNative.sendMail(accountId, folderId, form)`: validate + MIME + queue.
/// The composer's JSON form is `{to, cc?, bcc?, from?, from_name?,
/// reply_to?, subject, body, body_html?, attachments?, draft_uid?}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_sendMail<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    form: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::composer::send_mail(account_id, folder_id, string(env, &form)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.saveDraft(accountId, form)`: append the current text to
/// Drafts (created server-side when missing), replacing the opened version.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_saveDraft<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    form: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::composer::save_draft(account_id, string(env, &form)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.draftForm(accountId, uid)`: a stored draft back as an editable
/// form (attachments as metadata, never paths).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_draftForm<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::composer::draft_form(
                account_id,
                uid.max(0) as u32,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.deleteDraft(accountId, uid)`: destroy a server draft
/// (`\Deleted` + expunge) — what Discard means for a draft from Drafts.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteDraft<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    uid: i32,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::composer::delete_draft(account_id, uid.max(0) as u32)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.answerDraft(folderId, uid, mode)`: reply/reply-all/forward
/// draft JSON — `mode` is `reply`, `reply_all` or `forward`. Local read.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_answerDraft<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
    mode: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::composer::answer_draft(
                folder_id,
                uid.max(0) as u32,
                string(env, &mode)?,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.blankDraft()`: new-mail draft (just the signature), same
/// shape as `answerDraft`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_blankDraft<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::composer::blank_draft()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.imageDataUrl(path)`: an image file as a `data:` URL for
/// inline display. Throws for non-images and oversize files.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_imageDataUrl<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::composer::image_data_url(string(env, &path)?)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.isInlineImage(path)`: whether a file is offered inline (by
/// type) — `"true"`/`"false"`, like every other boolean here crosses as a
/// string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_isInlineImage<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(
                crate::api::composer::is_inline_image(string(env, &path)?).to_string(),
            )?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.senderParts(address)`: the From field split —
/// `{"local","domain"}`, domain locked with its `@`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_senderParts<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    address: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let p = crate::api::composer::sender_parts(string(env, &address)?);
            let json = serde_json::json!({
                "local": p.local,
                "domain": p.domain,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.effectiveFrom(local, accountEmail)`: the address a From field
/// sends as.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_effectiveFrom<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    local: JString<'caller>,
    account_email: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::composer::effective_from(
                string(env, &local)?,
                string(env, &account_email)?,
            ))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Step 0e search/contacts/settings/misc (`android/PLAN.md`): the last JNI
/// slice — everything the list, composer, contacts, settings, outbox and
/// reader screens need beyond 0a–0d. Same translation-only rule throughout.
///
/// `MailNative.searchJson(accountId, query, folder)`: FTS over
/// subject/sender/recipients/body; `folder` is an IMAP-path scope or `""`
/// for the account. Blank/operator-only queries answer `[]`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_searchJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    query: JString<'caller>,
    folder: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::search_json(
                account_id,
                string(env, &query)?,
                string(env, &folder)?,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.searchServer(accountId, query, folder)`: queued IMAP SEARCH
/// backfill into the cache; re-run `searchJson` when the `Search` job
/// finishes.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_searchServer<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    query: JString<'caller>,
    folder: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::search::search_server(
                account_id,
                string(env, &query)?,
                string(env, &folder)?,
            )?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.searchPlan(query)`: what the field does with its text —
/// `{"mode":"off"|"filter"|"indexed","query","hit_limit","debounce_ms"}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_searchPlan<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    query: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let p = crate::api::search::search_plan(string(env, &query)?);
            let mode = match p.mode {
                crate::api::search::SearchMode::Off => "off",
                crate::api::search::SearchMode::Filter => "filter",
                crate::api::search::SearchMode::Indexed => "indexed",
            };
            let json = serde_json::json!({
                "mode": mode,
                "query": p.query,
                "hit_limit": p.hit_limit,
                "debounce_ms": p.debounce_ms,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.searchFilterMatches(query, subject, from, fromName, snippet)`:
/// the short-input row filter — `"true"`/`"false"`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_searchFilterMatches<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    query: JString<'caller>,
    subject: JString<'caller>,
    from: JString<'caller>,
    from_name: JString<'caller>,
    snippet: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(
                crate::api::search::search_filter_matches(
                    string(env, &query)?,
                    string(env, &subject)?,
                    string(env, &from)?,
                    string(env, &from_name)?,
                    string(env, &snippet)?,
                )
                .to_string(),
            )?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.dateFilterMatches(dateRaw, after, before)`: one row's raw date
/// against an after-inclusive/before-exclusive `YYYY-MM-DD` pair.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_dateFilterMatches<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    date_raw: JString<'caller>,
    after: JString<'caller>,
    before: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(
                crate::api::search::date_filter_matches(
                    string(env, &date_raw)?,
                    string(env, &after)?,
                    string(env, &before)?,
                )
                .to_string(),
            )?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.datePresetRange(preset)`: `today`/`week`/`month`/`older_month`
/// as `{"after","before"}` (`""` = unset).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_datePresetRange<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    preset: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let r = crate::api::search::date_preset_range(string(env, &preset)?);
            let json = serde_json::json!({
                "after": r.after,
                "before": r.before,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.dateFilterLabel(after, before)`: the words for an active date
/// quick-filter.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_dateFilterLabel<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    after: JString<'caller>,
    before: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::date_filter_label(
                string(env, &after)?,
                string(env, &before)?,
            ))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.searchSyntaxHelp()`: the query-language tooltip text.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_searchSyntaxHelp<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::search_syntax_help())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.similarJson(accountId, folderId, uid)`: similar messages,
/// account-wide, best-first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_similarJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::similar_json(
                account_id,
                folder_id,
                uid.max(0) as i64,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.similarSubject(accountId, folderId, uid)`: the "Similar to"
/// chip text.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_similarSubject<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::search::similar_subject(
                account_id,
                folder_id,
                uid.max(0) as i64,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

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

/// `MailNative.settingsJson()`: every preference, normalized.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_settingsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::settings::settings_json()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.settingChoicesJson()`: defaults and offered values per key —
/// the settings screen labels these, in the shared words.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_settingChoicesJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::settings::setting_choices_json())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.quietTime(text)`: a typed time in picker parts —
/// `{"hour","minute"}`, `""` when it does not read as one.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_quietTime<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    text: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let out = match crate::api::settings::quiet_time(string(env, &text)?) {
                Some(t) => serde_json::json!({"hour": t.hour, "minute": t.minute}).to_string(),
                None => String::new(),
            };
            Ok(env.new_string(out)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.quietTimeAt(hour, minute)`: the stored `"HH:MM"` form of a
/// picked time, `""` when out of range.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_quietTimeAt<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    hour: i32,
    minute: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let out = crate::api::settings::quiet_time_at(
                u32::try_from(hour.max(0)).unwrap_or(u32::MAX),
                u32::try_from(minute.max(0)).unwrap_or(u32::MAX),
            )
            .unwrap_or_default();
            Ok(env.new_string(out)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setSetting(key, value)`: one preference; unknown keys throw
/// rather than persisting a typo.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setSetting<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    key: JString<'caller>,
    value: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::settings::set_setting(string(env, &key)?, string(env, &value)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

fn string_map_json(raw: &str) -> Result<Vec<(String, String)>> {
    let v: serde_json::Value = serde_json::from_str(raw)?;
    v.as_object()
        .ok_or_else(|| BridgeError("expected a JSON string object".to_string()))?
        .iter()
        .map(|(k, val)| {
            val.as_str()
                .map(|s| (k.clone(), s.to_string()))
                .ok_or_else(|| BridgeError(format!("bad value for {k}")))
        })
        .collect()
}

/// `MailNative.setSettings(values)`: several preferences from a JSON string
/// object, in one transaction — all apply or none do.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setSettings<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    values: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let pairs = string_map_json(&string(env, &values)?)?;
            crate::api::settings::set_settings(pairs.into_iter().collect())?;
            Ok(env.new_string("saved")?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setSort(field, descending)`: the list ordering, normalized as
/// a pair.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setSort<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    field: JString<'caller>,
    descending: bool,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::settings::set_sort(string(env, &field)?, descending)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.accountSettingsJson(accountId)`: one account's `overrides`
/// plus the `effective` values for every overridable key.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_accountSettingsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::settings::account_settings_json(account_id)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.setAccountSettings(accountId, values)`: one account's
/// overrides from a JSON string object — an empty value inherits again.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_setAccountSettings<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    values: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let pairs = string_map_json(&string(env, &values)?)?;
            crate::api::settings::set_account_settings(account_id, pairs.into_iter().collect())?;
            Ok(env.new_string("saved")?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.backgroundPlanJson()`: what the host should run — same plan
/// the scheduler parses, as JSON for the settings screen.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_backgroundPlanJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::settings::background_plan_json()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.attachmentsJson(folderId, uid)`: attachment metadata for one
/// message, standalone — no bytes.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_attachmentsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::messages::attachments_json(
                folder_id,
                uid.max(0) as u32,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.downloadAttachments(accountId, folderId, uid)`: queued
/// whole-message download; an `Attachments` finished event says when the
/// bytes are cached (unlike the blocking `downloadMessageFiles`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_downloadAttachments<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    uid: i32,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::attachments::download_attachments(
                account_id,
                folder_id,
                uid.max(0) as u32,
            )?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.saveAttachmentTo(attachmentId, path)`: a cached attachment to
/// a file or folder (which gets the safe name) — where it went. Fails
/// rather than downloading: call `downloadAttachments` first.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_saveAttachmentTo<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    attachment_id: i64,
    path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::attachments::save_attachment_to(
                attachment_id,
                string(env, &path)?,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.saveAllAttachmentsTo(folderId, uid, dir)`: every non-inline
/// attachment into `dir` — how many were saved, back as a string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_saveAllAttachmentsTo<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
    dir: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = crate::api::attachments::save_all_attachments_to(
                folder_id,
                uid.max(0) as u32,
                string(env, &dir)?,
            )?;
            Ok(env.new_string(n.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

fn reader_paint_name(p: crate::api::reader::ReaderPaint) -> &'static str {
    match p {
        crate::api::reader::ReaderPaint::Theme => "theme",
        crate::api::reader::ReaderPaint::Original => "original",
        crate::api::reader::ReaderPaint::Darkened => "darkened",
    }
}

fn parse_paint(raw: &str) -> Result<crate::api::reader::ReaderPaint> {
    use crate::api::reader::ReaderPaint;
    match raw {
        "theme" => Ok(ReaderPaint::Theme),
        "original" => Ok(ReaderPaint::Original),
        "darkened" => Ok(ReaderPaint::Darkened),
        _ => Err(BridgeError(format!("bad paint: {raw}"))),
    }
}

/// `MailNative.readerPaint(colored, dark, keepOriginal)`: the paint for one
/// mail — `"theme"`, `"original"` or `"darkened"`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerPaint<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    colored: bool,
    dark: bool,
    keep_original: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(
                reader_paint_name(crate::api::reader::reader_paint(
                    colored,
                    dark,
                    keep_original,
                ))
                .to_string(),
            )?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerPalette(paint, paper, ink, link, quote, rule)`: what
/// `paint` writes the page in — `{"paper","ink","link","quote","rule"}` as
/// `0xRRGGBB` ints.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerPalette<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    paint: JString<'caller>,
    paper: i32,
    ink: i32,
    link: i32,
    quote: i32,
    rule: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let rgb = |v: i32| (v as u32) & 0xFF_FFFF;
            let pal = crate::api::reader::reader_palette(
                parse_paint(&string(env, &paint)?)?,
                crate::api::reader::ReaderPalette {
                    paper: rgb(paper),
                    ink: rgb(ink),
                    link: rgb(link),
                    quote: rgb(quote),
                    rule: rgb(rule),
                },
            );
            let json = serde_json::json!({
                "paper": pal.paper,
                "ink": pal.ink,
                "link": pal.link,
                "quote": pal.quote,
                "rule": pal.rule,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerFitBelow(body)`: pages narrower than this get fixed
/// widths loosened — back as a string, `0` when the mail has none.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerFitBelow<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    body: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(
                crate::api::reader::reader_fit_below(string(env, &body)?).to_string(),
            )?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerBody(body, paint, fit)`: the body as `paint` shows it,
/// for a renderer that takes a body rather than a document.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerBody<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    body: JString<'caller>,
    paint: JString<'caller>,
    fit: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::reader::reader_body(
                string(env, &body)?,
                parse_paint(&string(env, &paint)?)?,
                fit,
            ))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.readerDocumentFull(body, paint, paper, ink, link, quote,
/// rule, allowRemote, topSpace, scale, fit)`: the full document with every
/// option — unlike `readerDocument`, which fixes `top_space: 0` for the
/// Views experiment layout.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readerDocumentFull<'caller>(
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
    top_space: i32,
    scale: f32,
    fit: bool,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let rgb = |v: i32| (v as u32) & 0xFF_FFFF;
            Ok(env.new_string(crate::api::reader::reader_document(
                string(env, &body)?,
                crate::api::reader::ReaderDocumentOptions {
                    paint: parse_paint(&string(env, &paint)?)?,
                    theme: crate::api::reader::ReaderPalette {
                        paper: rgb(paper),
                        ink: rgb(ink),
                        link: rgb(link),
                        quote: rgb(quote),
                        rule: rgb(rule),
                    },
                    allow_remote,
                    top_space: u32::try_from(top_space.max(0)).unwrap_or(u32::MAX),
                    scale: f64::from(scale),
                    fit,
                },
            ))?)
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

/// `MailNative.storageStatsJson(dbPath, tempDir)`: database, message and
/// temp sizes for the maintenance section.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_storageStatsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    db_path: JString<'caller>,
    temp_dir: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::maintenance::storage_stats_json(
                string(env, &db_path)?,
                string(env, &temp_dir)?,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.cleanupTempFilesJson(tempDir)`: delete temp files now —
/// `{files_removed, bytes_freed, ...}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_cleanupTempFilesJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    temp_dir: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(
                env.new_string(crate::api::maintenance::cleanup_temp_files_json(string(
                    env, &temp_dir,
                )?)?)?,
            )
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.trimLocalCache()`: trim the local cache to the newest rows —
/// how many went, back as a string.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_trimLocalCache<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::maintenance::trim_local_cache()?.to_string())?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.trimStatus(removed)`: the words for a trim result.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_trimStatus<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    removed: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let n = u64::try_from(removed)
                .map_err(|_| BridgeError(format!("bad trim count: {removed}")))?;
            Ok(env.new_string(crate::api::maintenance::trim_status(n))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.evictCachedAttachmentsJson()`: drop downloaded attachment
/// bytes (they re-download on open) — `{files, bytes_freed, ...}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_evictCachedAttachmentsJson<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::maintenance::evict_cached_attachments_json()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.exportDatabaseTo(path)`: a `VACUUM INTO` snapshot of the
/// database at `path` — where it went.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_exportDatabaseTo<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(
                env.new_string(crate::api::maintenance::export_database_to(string(
                    env, &path,
                )?)?)?,
            )
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
