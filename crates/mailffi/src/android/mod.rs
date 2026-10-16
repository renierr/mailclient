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
//!
//! One domain per submodule: [`jobs`] (the job-event listener and busy
//! state), [`checks`] (background checks and notification actions),
//! [`push`] (the IDLE monitor), then one module per screen family —
//! [`accounts`], [`folders`], [`sync`], [`list`], [`mutate`], [`search`],
//! [`reader`], [`composer`], [`outbox`], [`attachments`], [`export`],
//! [`contacts`], [`settings`] and [`maintenance`].

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::{Env, EnvUnowned};

mod accounts;
mod attachments;
mod checks;
mod composer;
mod contacts;
mod export;
mod folders;
mod jobs;
mod list;
mod maintenance;
mod mutate;
mod outbox;
mod push;
mod reader;
mod search;
mod settings;
mod sync;
pub(crate) use jobs::{forward_busy, forward_job_event};

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

/// A UID from Kotlin's `Int`. Negative is refused rather than clamped: a
/// `-1` used to become an operation on UID 0 instead of an error (E6). UIDs
/// past `Int.MAX_VALUE` cannot cross as `Int` at all; that is D3's widening.
fn uid_arg(uid: i32) -> Result<u32> {
    u32::try_from(uid).map_err(|_| BridgeError(format!("invalid uid {uid}")))
}

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

/// Drive `f` on a throwaway current-thread Tokio runtime, like
/// `headless::push_changes_blocking`: JNI worker threads have no runtime.
fn blocking<T>(f: impl std::future::Future<Output = anyhow::Result<T>>) -> Result<T> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BridgeError(format!("runtime: {e}")))?;
    rt.block_on(f).map_err(|e| BridgeError(format!("{e:#}")))
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
