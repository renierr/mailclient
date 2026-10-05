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

use std::collections::HashSet;
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

/// Jobs currently on the thread, keyed by [`spawn`]'s `key`.
///
/// Deliberately not `mailapp`'s single `busy` flag: there, one latch guards a
/// GUI that shows one spinner. Here a sync in flight must not refuse an
/// attachment download, so only a *second job of the same kind* is refused.
fn inflight() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Keys of jobs currently on the network thread, for diagnostics (the dev
/// probe shows them): a job listed here started but never reported back.
pub(crate) fn inflight_keys() -> Vec<String> {
    let mut keys: Vec<String> = inflight()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .cloned()
        .collect();
    keys.sort();
    keys
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
    {
        let mut set = inflight().lock().unwrap_or_else(|e| e.into_inner());
        if !set.insert(key.clone()) {
            anyhow::bail!("{kind} is already running");
        }
    }
    let kind = kind.to_string();
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
        inflight()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&key);
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
        inflight()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&key_if_undelivered);
        anyhow::bail!("the network thread is not running");
    }
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

/// [`spawn_flag_push`] once an undoable action's grace period is over. The
/// wait runs on its own thread so the net thread stays free; if the app is
/// closed first, the queued move is pushed by the next sync instead.
pub(crate) fn spawn_push_after_grace(account_id: i64) {
    std::thread::spawn(move || {
        let secs = mailcore::undo::UNDO_GRACE_SECS.max(0) as u64 + 1;
        std::thread::sleep(std::time::Duration::from_secs(secs));
        spawn_flag_push(account_id);
    });
}
