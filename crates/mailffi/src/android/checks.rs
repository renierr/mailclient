//! Background checks, the notification plan and its actions.

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;
use mailcore::sync::background::{self, notify, BackgroundReport, SeenMark};
use mailcore::sync::headless;
use serde::Deserialize;

/// `MailNative.check(trigger, now)`: one check (sync every inbox, then
/// report), as the `BackgroundReport` JSON. Blocks for the network run, so
/// only a worker thread calls it. `now` is the explicit check
/// (`background_check_now`): every account, quiet hours included — what the
/// "Check mail" Quick Settings tile taps.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_check<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    trigger: JString<'caller>,
    now: jni::sys::jboolean,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let trigger = string(env, &trigger)?;
            let db = crate::db::shared_db()?;
            let report = if now {
                background::background_check_now_blocking(db, &crate::db::db_path(), &trigger)
            } else {
                background::background_check_blocking(db, &crate::db::db_path(), &trigger)
            };
            Ok(env.new_string(serde_json::to_string(&report)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.backgroundPlan()`: what the host should run now, as the
/// `BackgroundPlan` JSON plus `any`. Database only, no network; called again at the
/// plan's `replan_at`, when some account's quiet hours start or end.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_backgroundPlan<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let plan = background::schedule::plan(crate::db::shared_db()?);
            Ok(env.new_string(serde_json::to_string(&plan.view())?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.plan(report, permitted, foreground, shown)`: what to do with
/// the notifications for a report, as `NotificationPlan` JSON. `shown` is a
/// JSON object of the app's notifications on screen, tag →
/// `{"title", "body"}` as read back from each ([`notify::shown_of`] builds
/// the signatures).
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
            let shown = notify::shown_of(serde_json::from_str(&string(env, &shown)?)?);
            let db = crate::db::shared_db()?;
            let plan = notify::plan_for(db, &report, permitted, foreground, &shown);
            Ok(env.new_string(serde_json::to_string(&plan)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.markRead(target)`: a notification's "Mark read" button, with
/// the `ReadTarget` JSON the plan gave it. Marks the cache only and returns
/// a `BackgroundReport` of what is still pending, to plan again with;
/// `pushChanges` carries the change to the server.
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

/// `MailNative.readTargetAccount(target)`: the account of a "Mark read"
/// button's `ReadTarget` JSON.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_readTargetAccount<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    target: JString<'caller>,
) -> i64 {
    unowned
        .with_env(|env| -> Result<i64> {
            Ok(notify::ReadTarget::account_of(&string(env, &target)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.notifyAct(target, action)`: a notification's Archive or
/// Delete button (`action` is its plan's `quick_action`). Queues the move
/// with no undo window and returns a `BackgroundReport` of what is still
/// pending, to plan again with; `pushChanges` carries it to the server.
/// Throws when the delete would destroy the mail.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_notifyAct<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    target: JString<'caller>,
    action: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let target: notify::ReadTarget = serde_json::from_str(&string(env, &target)?)?;
            let report = notify::act(crate::db::shared_db()?, &target, &string(env, &action)?)
                .map_err(BridgeError)?;
            Ok(env.new_string(serde_json::to_string(&report)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.notifyReply(target, text)`: a notification's Reply. The core
/// builds and queues the reply (`notify::quick_reply`); delivery runs on
/// `mailclient-net` like a composer send — the pooled sessions it files
/// the Sent copy over are only driven from there — and this blocks until it
/// is done, so only a worker thread calls it. Returns the job's status;
/// throws when the reply was not sent (the outbox shows it as failed).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_notifyReply<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    target: JString<'caller>,
    text: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let target: notify::ReadTarget = serde_json::from_str(&string(env, &target)?)?;
            let db = crate::db::shared_db()?;
            let prepared =
                notify::quick_reply(db, &target, &string(env, &text)?).map_err(BridgeError)?;
            let queue_id = prepared.queue_id;
            let (tx, rx) = std::sync::mpsc::channel::<std::result::Result<String, String>>();
            let started = crate::net::spawn(
                "Send",
                format!("send:{queue_id}"),
                move |db, progress| async move {
                    let delivered = mailcore::compose::deliver(db, prepared, -1, || {
                        progress.report("");
                    })
                    .await;
                    let done = delivered.map(|outcome| crate::net::JobDone {
                        status: if outcome.notes.is_empty() {
                            "Reply sent".to_string()
                        } else {
                            outcome.notes.join("; ")
                        },
                        refresh: Some(crate::net::JobRefresh::account(outcome.account_id)),
                        outcome: outcome.outcome().to_string(),
                    });
                    let _ = tx.send(
                        done.as_ref()
                            .map(|d| d.status.clone())
                            .map_err(Clone::clone),
                    );
                    done
                },
            );
            if let Err(e) = started {
                mailcore::compose::abandon_send(db, queue_id);
                return Err(e.into());
            }
            let status = match rx.recv_timeout(std::time::Duration::from_secs(300)) {
                Ok(result) => result.map_err(BridgeError)?,
                // Still going: the job finishes on its own and the outbox
                // shows how.
                Err(_) => "Still sending".to_string(),
            };
            Ok(env.new_string(status)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.pushChanges(accountId)`: send the account's queued flag
/// changes and due moves over a fresh connection. Blocks for the network, so
/// only a worker thread calls it; throws when the server cannot be reached
/// (the changes stay queued).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_pushChanges<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            let db = crate::db::shared_db()?;
            headless::push_changes_blocking(db, account_id)?;
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

/// `MailNative.backgroundRunLines()`: the last run and the recent runs in
/// words, local time (`background::describe::run_lines`), as
/// `{last, history}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_backgroundRunLines<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let runs = background::run_history(crate::db::shared_db()?);
            let lines = background::describe::run_lines(&runs);
            Ok(env.new_string(serde_json::to_string(&lines)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.limitingBucket(bucket)`: the name of a standby bucket that
/// limits background checks, `""` for one that does not.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_limitingBucket<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    bucket: i32,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let name = background::describe::limiting_bucket(bucket).unwrap_or("");
            Ok(env.new_string(name)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.heartbeatGap(secs)`: "every 45 seconds", "every 4 minutes".
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_heartbeatGap<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    secs: i64,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(background::describe::heartbeat_gap(secs))?)
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
