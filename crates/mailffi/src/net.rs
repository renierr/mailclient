//! The `mailclient-net` thread: every IMAP/SMTP job runs here, never inline.
//!
//! `mailapp` queues onto a dedicated thread holding a current-thread Tokio
//! runtime and hears back through `CxxQtThread`. The shape is the same here,
//! with the Dart event stream ([`crate::api::events`]) standing in for the Qt
//! signal. flutter_rust_bridge would happily run a call on its worker pool,
//! but one shared thread is what makes the pooled IMAP sessions in
//! [`mailcore::sync::pool`] sound: they are checked out under a mutex and are
//! not safe to drive from two places at once.
//!
//! The difference from the Qt worker: a finished job reports *what changed*,
//! not a rebuilt feed. Dart owns the selection, so it re-reads whatever it is
//! currently showing — which is why there is no stale-selection reconciliation
//! here.

use std::collections::HashMap;
use std::sync::{mpsc, Mutex, OnceLock};

use crate::api::events::{emit_event, JobEvent, JobPhase};
use crate::db::shared_db;
use mailcore::sync::pool::guard;

type JobFn = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send>;

/// What a finished job invalidated, so Dart knows what to re-read.
///
/// `None` from a job means "nothing the UI shows changed" — reading a draft
/// or saving an attachment must not make the message list rebuild itself.
#[derive(Clone, Copy, Default)]
pub(crate) struct JobRefresh {
    pub account_id: i64,
    /// `-1` = every folder of the account (a full sync), not "no folder".
    pub folder_id: i64,
}

impl JobRefresh {
    pub fn account(account_id: i64) -> Self {
        Self {
            account_id,
            folder_id: -1,
        }
    }
    pub fn folder(account_id: i64, folder_id: i64) -> Self {
        Self {
            account_id,
            folder_id,
        }
    }
}

/// What a finished job reports: status prose plus a machine-readable
/// `outcome` Dart keys decisions off instead of matching the status text.
pub(crate) struct JobDone {
    pub status: String,
    pub refresh: Option<JobRefresh>,
    pub outcome: String,
}

impl From<(String, Option<JobRefresh>)> for JobDone {
    fn from((status, refresh): (String, Option<JobRefresh>)) -> Self {
        Self {
            status,
            refresh,
            outcome: String::new(),
        }
    }
}

/// Lets a running job report a milestone before it is finished.
///
/// A send is the case that matters: once SMTP has accepted the message it
/// *is* sent, and keeping the composer open while the Sent copy is appended
/// and the folder resyncs is a lie about what the user is waiting for.
pub(crate) struct JobProgress {
    kind: String,
}

impl JobProgress {
    pub fn report(&self, status: &str) {
        emit_event(JobEvent {
            kind: self.kind.clone(),
            phase: JobPhase::Progress,
            status: status.to_string(),
            account_id: -1,
            folder_id: -1,
            ok: true,
            outcome: String::new(),
        });
    }
}

/// Jobs queued or running on the thread: [`spawn`]'s `key` → job kind.
///
/// Deliberately not `mailapp`'s single `busy` flag: there, one latch guards a
/// GUI that shows one spinner. Here a sync in flight must not refuse an
/// attachment download, so only a *second job of the same key* is refused.
///
/// `generation` counts every insert and removal, so a frontend that hears
/// about the table from two threads (the caller's queue thread and the net
/// thread) can drop a snapshot older than one it already applied.
#[derive(Default)]
struct Inflight {
    jobs: HashMap<String, String>,
    generation: u64,
}

fn inflight() -> &'static Mutex<Inflight> {
    static SET: OnceLock<Mutex<Inflight>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(Inflight::default()))
}

/// What is queued or running right now, as the frontend's busy indicator
/// should show it. Read from the core's own dedupe table, so it cannot
/// drift from what the net thread actually does.
#[derive(serde::Serialize)]
pub(crate) struct BusySnapshot {
    pub generation: u64,
    /// Distinct job kinds in flight (`"Sync"`, `"Folders"`, …), sorted.
    pub kinds: Vec<String>,
    /// Dedupe keys in flight, sorted — diagnostics only.
    pub keys: Vec<String>,
}

// Only the JNI side reads it; Dart keeps its own `_busyKinds`.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn busy_snapshot() -> BusySnapshot {
    let set = inflight().lock().unwrap_or_else(|e| e.into_inner());
    snapshot_of(&set)
}

fn snapshot_of(set: &Inflight) -> BusySnapshot {
    let mut keys: Vec<String> = set.jobs.keys().cloned().collect();
    keys.sort();
    let mut kinds: Vec<String> = set.jobs.values().cloned().collect();
    kinds.sort();
    kinds.dedup();
    BusySnapshot {
        generation: set.generation,
        kinds,
        keys,
    }
}

/// Whether a job with dedupe `key` is queued or running. Asked by the
/// Android adapter only.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) fn is_inflight(key: &str) -> bool {
    inflight()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .jobs
        .contains_key(key)
}

/// The dedupe key of one message's attachment download.
pub(crate) fn attachments_key(folder_id: i64, uid: u32) -> String {
    format!("attach:{folder_id}:{uid}")
}

fn remove_inflight(key: &str) {
    let mut set = inflight().lock().unwrap_or_else(|e| e.into_inner());
    if set.jobs.remove(key).is_some() {
        set.generation += 1;
    }
}

fn net_tx() -> &'static mpsc::Sender<JobFn> {
    static TX: OnceLock<mpsc::Sender<JobFn>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<JobFn>();
        std::thread::Builder::new()
            .name("mailclient-net".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime for mailclient-net");
                let _guard = rt.enter();
                while let Ok(job) = rx.recv() {
                    // A panic escaping a job would unwind this thread out of
                    // existence and every later job would be dropped by the
                    // `let _ =` at the call sites, with nothing logged. Jobs
                    // that can report their own failure wrap themselves too
                    // (see `spawn`); this catches the ones that cannot.
                    let _ = guard("background job", || {
                        job(&rt);
                        Ok::<(), String>(())
                    });
                }
            })
            .expect("mailclient-net thread");
        tx
    })
}

/// Queue `op` on the network thread. Returns as soon as it is queued.
///
/// `key` dedupes: a second job with a key already in flight is refused rather
/// than stacked, so holding down ⟳ cannot open five IMAP sessions. Completion
/// arrives on the Dart event stream as a [`JobPhase::Finished`] event.
pub(crate) fn spawn<F, Fut, D>(kind: &str, key: String, op: F) -> anyhow::Result<()>
where
    F: FnOnce(&'static mailcore::Db, JobProgress) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<D, String>> + 'static,
    D: Into<JobDone>,
{
    #[cfg_attr(not(target_os = "android"), allow(unused_variables))]
    let queued = {
        let mut set = inflight().lock().unwrap_or_else(|e| e.into_inner());
        if set.jobs.contains_key(&key) {
            anyhow::bail!("{kind} is already running");
        }
        set.jobs.insert(key.clone(), kind.to_string());
        set.generation += 1;
        snapshot_of(&set)
    };
    let kind = kind.to_string();
    #[cfg(target_os = "android")]
    let queued_kind = kind.clone();
    let progress = JobProgress { kind: kind.clone() };
    // The closure owns the key (it clears the entry when the job ends); the
    // failure path below needs it too, so it keeps its own copy.
    let key_if_undelivered = key.clone();
    let sent = net_tx().send(Box::new(move |rt| {
        let outcome = guard(&kind, || {
            rt.block_on(async {
                let db = shared_db().map_err(|e| e.to_string())?;
                op(db, progress).await.map(|d| -> JobDone { d.into() })
            })
        });
        remove_inflight(&key);
        let (status, refresh, job_outcome, ok) = match outcome {
            Ok(done) => (done.status, done.refresh, done.outcome, true),
            Err(e) => (e, None, String::new(), false),
        };
        let refresh = refresh.unwrap_or(JobRefresh {
            account_id: -1,
            folder_id: -1,
        });
        emit_event(JobEvent {
            kind,
            phase: JobPhase::Finished,
            status,
            account_id: refresh.account_id,
            folder_id: refresh.folder_id,
            ok,
            outcome: job_outcome,
        });
    }));
    if sent.is_err() {
        // Only reachable if the net thread died despite the guards above.
        remove_inflight(&key_if_undelivered);
        anyhow::bail!("the network thread is not running");
    }
    // Kotlin keeps no busy bookkeeping of its own: it hears that the job is
    // in flight from here, the same way it hears that it ended. A finish
    // that overtakes this carries a newer generation, so the stale snapshot
    // is dropped on arrival.
    #[cfg(target_os = "android")]
    crate::android::forward_busy(&queued_kind, &queued);
    Ok(())
}

/// Fire-and-forget push of locally-dirtied read/star flags.
///
/// Runs after every toggle so Seen reaches the server within seconds instead
/// of waiting for the next full sync — quitting right after reading loses
/// nothing. Deliberately outside [`spawn`]: no dedupe key (a slow network
/// must never make the next click fail), no event (the UI already shows the
/// local change). Exits without touching the network when nothing is dirty;
/// failures stay dirty for the next regular sync.
pub(crate) fn spawn_flag_push(account_id: i64) {
    let _ = net_tx().send(Box::new(move |rt| {
        rt.block_on(async {
            match shared_db() {
                Ok(db) => {
                    mailcore::undo::push_local_changes(db, account_id).await;
                }
                Err(e) => log::warn!("local-push: cannot open db: {e}"),
            }
        });
    }));
}

/// [`spawn_flag_push`] once an undoable action's grace period is over, on
/// `mailcore`'s one shared timer thread (not a thread per action); if the
/// app is closed first, the queued move is pushed by the next sync instead.
pub(crate) fn spawn_push_after_grace(account_id: i64) {
    mailcore::undo::push_after_grace(account_id, spawn_flag_push);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_lists_each_kind_once_sorted() {
        let mut set = Inflight::default();
        set.jobs.insert("sync:1".into(), "Sync".into());
        set.jobs.insert("sync-folder:7".into(), "Sync".into());
        set.jobs.insert("folders:1".into(), "Folders".into());
        set.generation = 3;
        let snap = snapshot_of(&set);
        assert_eq!(snap.generation, 3);
        assert_eq!(snap.kinds, vec!["Folders", "Sync"]);
        assert_eq!(snap.keys, vec!["folders:1", "sync-folder:7", "sync:1"]);
    }
}
