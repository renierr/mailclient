//! Background network jobs. IMAP/SMTP never run on the Qt GUI thread.

use std::pin::Pin;
use std::sync::{mpsc, OnceLock};

use cxx_qt::Threading;
use cxx_qt_lib::QString;

use crate::bridge::qobject;
use crate::bridge::{feed_epoch, push_feeds, qstring, shared_db, Feeds};
use mailcore::sync::pool::guard;

/// What to refresh on the GUI after a job.
#[derive(Clone, Copy)]
pub(crate) struct JobRefresh {
    pub account_id: i64,
    pub folder_id: i64,
    pub message_limit: Option<i32>,
}

/// The selection a job started from, so its completion can tell a deliberate
/// redirect (the synced folder vanished) from a stale one (the user moved on
/// while the network was busy).
#[derive(Clone, Copy)]
struct Selection {
    account_id: i64,
    folder_id: i64,
}

impl JobRefresh {
    /// Rebuild the folder and message feeds for this selection.
    pub fn feeds(account_id: i64, folder_id: i64) -> Self {
        Self {
            account_id,
            folder_id,
            message_limit: None,
        }
    }

    /// Resolve what the GUI should actually show. A job that asks for the same
    /// selection it started on is only restating it, so the user's newer
    /// choice wins; a job that asks for something else redirected on purpose
    /// and is honoured — unless the account changed underneath it, which makes
    /// its whole view stale.
    fn resolve(self, started: Selection, live: Selection) -> Selection {
        if live.account_id != started.account_id {
            return live;
        }
        let folder_id = if self.folder_id == started.folder_id {
            live.folder_id
        } else {
            self.folder_id
        };
        Selection {
            account_id: self.account_id,
            folder_id,
        }
    }
}

/// Lets a running job tell the GUI it has passed a milestone worth acting on
/// before the job is over. A send is the case that matters: once SMTP has
/// accepted the message it *is* sent, and making the user watch the composer
/// while the Sent copy is appended and the folder resyncs is a lie about what
/// they are waiting for.
pub(crate) struct JobProgress {
    qt: cxx_qt::CxxQtThread<qobject::Bridge>,
    kind: String,
}

impl JobProgress {
    pub fn report(&self, status: &str) {
        let kind = self.kind.clone();
        let status = status.to_string();
        if let Err(e) = self.qt.queue(move |mut bridge| {
            let kind = qstring(&kind);
            let status = qstring(&status);
            bridge.as_mut().job_progress(&kind, &status);
        }) {
            log::warn!("worker: cannot report job progress to the GUI thread: {e}");
        }
    }
}

/// What a refused [`spawn_job`] returns while another job holds the latch.
pub(crate) const BUSY_MESSAGE: &str = "busy — wait for the current action";

/// What a finished job reports: status prose for the status bar, what feeds
/// to rebuild, and a machine-readable `outcome` QML keys decisions off
/// (`SendOutcome::outcome`; `""` for jobs without one) instead of matching
/// the status text.
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

type JobFn = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send>;

/// Fire-and-forget push of local changes — read/star toggles and undoable
/// moves whose grace period is over (see `mailcore::undo`). Run after every
/// toggle so Seen reaches the server within seconds instead of waiting for
/// the next full sync — quitting right after reading loses nothing.
///
/// Deliberately outside [`spawn_job`]: no busy latch (a slow network must
/// never block the next click), no status line, no feed rebuild (the feeds
/// already show the local change). Exits early without touching the network
/// when nothing is queued. Failures stay queued for the next regular sync.
pub(crate) fn spawn_flag_push(account_id: i64) {
    let tx = match net() {
        Ok(tx) => tx,
        Err(reason) => {
            log::warn!("local-push: {reason}");
            return;
        }
    };
    let _ = tx.send(Box::new(move |rt| {
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
/// app quits first, the queued move is pushed by the next sync instead.
pub(crate) fn spawn_push_after_grace(account_id: i64) {
    mailcore::undo::push_after_grace(account_id, spawn_flag_push);
}

/// The one background thread that runs every network job, and the channel
/// that feeds it, once [`net`] has tried to bring them up.
enum Net {
    /// The thread is running; jobs may be queued onto it.
    Ready(mpsc::Sender<JobFn>),
    /// Bringing the thread up failed. Sticky — see [`net`].
    Failed(String),
}

static NET: OnceLock<Net> = OnceLock::new();

/// The queue feeding the `mailclient-net` thread, or why there is none.
///
/// A failure is sticky on purpose. Both failure modes (the OS refusing a
/// thread, the runtime refusing to build) are permanent for the process, and
/// retrying them on every click would spawn a thread each time in the
/// pathological case. Callers must report the reason instead of panicking or
/// latching `busy`, so the difference between "try again in a moment" and
/// "this app will never reach the network" reaches the user.
pub(crate) fn net() -> Result<&'static mpsc::Sender<JobFn>, &'static str> {
    match NET.get_or_init(Net::start) {
        Net::Ready(tx) => Ok(tx),
        Net::Failed(reason) => Err(reason),
    }
}

impl Net {
    fn start() -> Self {
        let (tx, rx) = mpsc::channel::<JobFn>();
        // The runtime is built *inside* the thread rather than here on the
        // caller's thread. The caller is the GUI thread (this runs on the
        // first job queued from a QML click), and `expect` on a GUI thread
        // is an abort the window cannot catch. Building inside means a
        // failure is a log line plus a `Failed` the user gets to read.
        let spawned = std::thread::Builder::new()
            .name("mailclient-net".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => {
                        let _guard = rt.enter();
                        while let Ok(job) = rx.recv() {
                            // A panic escaping a job would unwind this thread out
                            // of existence, and every later `send` would then be
                            // dropped by the `let _ =` at the call sites -- sync,
                            // send and downloads would silently stop working
                            // until restart, with nothing logged. Jobs that can
                            // report a failure to the user wrap themselves too
                            // (see `spawn_job`); this is the net that catches the
                            // ones that cannot.
                            let _ = guard("background job", || {
                                job(&rt);
                                Ok::<(), String>(())
                            });
                        }
                    }
                    Err(e) => log::error!("worker: cannot build the net runtime: {e}"),
                }
            });
        match spawned {
            Ok(_thread) => Net::Ready(tx),
            Err(e) => Net::Failed(format!("cannot start the net thread: {e}")),
        }
    }
}

/// What [`spawn_job`] may proceed with: the queue to send the job on, or the
/// message to show instead.
///
/// A running job outranks a missing net thread: its message reads as "try in
/// a moment", which is the more likely of the two from the user's chair. The
/// net-thread reason is only surfaced once the latch is free, and it is never
/// reported as the busy message — the two need different reactions, and
/// latching `busy` for a job that cannot run would refuse every later click
/// until restart.
fn entry_gate<'a>(
    busy: bool,
    net: Result<&'a mpsc::Sender<JobFn>, &str>,
) -> Result<&'a mpsc::Sender<JobFn>, String> {
    if busy {
        return Err(BUSY_MESSAGE.to_string());
    }
    match net {
        Ok(tx) => Ok(tx),
        Err(reason) => Err(format!("network is unavailable: {reason}")),
    }
}

/// Run `op` on the network thread. Returns immediately with `""` (queued) or
/// a busy message. Completion is [`qobject::Bridge::job_finished`]; `op`
/// returns `None` for the refresh when it changed nothing the feeds show, so
/// reading a draft or saving an attachment does not rebuild the message list.
pub(crate) fn spawn_job<F, Fut, D>(
    mut bridge: Pin<&mut qobject::Bridge>,
    kind: &str,
    op: F,
) -> QString
where
    F: FnOnce(&'static mailcore::Db, JobProgress) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<D, String>> + 'static,
    D: Into<JobDone>,
{
    // Everything that can refuse does so *before* `busy` is latched, so a job
    // that will never run cannot leave the latch stuck and refuse every later
    // click until restart.
    let tx = match entry_gate(*bridge.busy(), net()) {
        Ok(tx) => tx,
        Err(message) => return qstring(&message),
    };
    bridge.as_mut().set_busy(true);
    let started = Selection {
        account_id: *bridge.current_account_id(),
        folder_id: *bridge.current_folder_id(),
    };
    let qt = bridge.qt_thread();
    let kind_owned = kind.to_string();
    let progress = JobProgress {
        qt: qt.clone(),
        kind: kind_owned.clone(),
    };
    let _ = tx.send(Box::new(move |rt| {
        let outcome = guard(&kind_owned, || {
            rt.block_on(async {
                let db = shared_db()?;
                op(db, progress).await.map(|d| -> JobDone { d.into() })
            })
        });
        let (status, refresh, job_outcome) = match outcome {
            Ok(done) => (done.status, done.refresh, done.outcome),
            Err(e) => (e, None, String::new()),
        };
        // The whole folder as JSON is the expensive part of a refresh, so it
        // is built here rather than on the GUI thread. It is a guess at what
        // the GUI will show: used only when the selection resolves to it and
        // nothing local changed the feeds since (`feed_epoch`).
        let prebuilt = refresh.and_then(|r| {
            let epoch = feed_epoch();
            let db = shared_db().ok()?;
            Some((epoch, Feeds::build(db, r.account_id, r.folder_id)))
        });
        let queued = qt.queue(move |mut bridge| {
            if let Some(refresh) = refresh {
                if let Some(limit) = refresh.message_limit {
                    bridge.as_mut().set_message_limit(limit);
                }
                let live = Selection {
                    account_id: *bridge.current_account_id(),
                    folder_id: *bridge.current_folder_id(),
                };
                let target = refresh.resolve(started, live);
                match prebuilt {
                    Some((epoch, feeds))
                        if epoch == feed_epoch()
                            && feeds.shows(target.account_id, target.folder_id) =>
                    {
                        feeds.apply(&mut bridge);
                    }
                    _ => {
                        if let Ok(db) = shared_db() {
                            push_feeds(&mut bridge, db, target.account_id, target.folder_id);
                        }
                    }
                }
            }
            bridge.as_mut().set_busy(false);
            let kind = qstring(&kind_owned);
            let status = qstring(&status);
            let outcome = qstring(&job_outcome);
            bridge.job_finished(&kind, &status, &outcome);
        });
        if let Err(e) = queued {
            // Only reachable once the QObject is gone, i.e. during shutdown —
            // but `busy` would stay latched forever if it ever happened while
            // the window still lived, so never let it pass silently.
            log::error!("worker: cannot deliver job result to the GUI thread: {e}");
        }
    }));
    qstring("")
}

#[cfg(test)]
mod tests {
    use super::{entry_gate, net, JobFn, JobRefresh, Selection, BUSY_MESSAGE};
    use std::sync::mpsc;

    fn sel(account_id: i64, folder_id: i64) -> Selection {
        Selection {
            account_id,
            folder_id,
        }
    }

    #[test]
    fn a_folder_switch_during_the_job_is_not_undone() {
        let started = sel(1, 10);
        let refresh = JobRefresh::feeds(1, 10);
        let got = refresh.resolve(started, sel(1, 20));
        assert_eq!(got.folder_id, 20);
    }

    #[test]
    fn a_job_that_redirects_still_wins() {
        // The synced folder disappeared, so the job picked the inbox instead.
        let started = sel(1, 10);
        let refresh = JobRefresh::feeds(1, 99);
        let got = refresh.resolve(started, sel(1, 10));
        assert_eq!(got.folder_id, 99);
    }

    #[test]
    fn an_account_switch_discards_the_whole_stale_view() {
        let started = sel(1, 10);
        let refresh = JobRefresh::feeds(1, 99);
        let got = refresh.resolve(started, sel(2, 30));
        assert_eq!((got.account_id, got.folder_id), (2, 30));
    }

    /// A channel nobody reads from, just to hold a `Sender` for the tests
    /// below without starting a thread.
    fn idle_channel() -> (mpsc::Sender<JobFn>, mpsc::Receiver<JobFn>) {
        mpsc::channel::<JobFn>()
    }

    #[test]
    fn a_live_net_thread_lets_an_idle_click_through() {
        let (tx, _rx) = idle_channel();
        assert!(entry_gate(false, Ok(&tx)).is_ok());
    }

    #[test]
    fn a_running_job_outranks_a_missing_net_thread() {
        let (tx, _rx) = idle_channel();
        assert_eq!(
            entry_gate(true, Ok(&tx)).expect_err("busy wins"),
            BUSY_MESSAGE
        );
        // Both conditions at once still reports the transient one.
        assert_eq!(
            entry_gate(true, Err("cannot start the net thread: nope")).expect_err("busy wins"),
            BUSY_MESSAGE
        );
    }

    #[test]
    fn a_dead_net_thread_is_reported_as_something_other_than_busy() {
        // A job refused for a missing net thread must not read as the busy
        // message: the user's next click would then be refused too, and the
        // only way out would be a restart.
        let message =
            entry_gate(false, Err("cannot start the net thread: nope")).expect_err("must refuse");
        assert_ne!(message, BUSY_MESSAGE);
        assert!(
            message.contains("cannot start the net thread"),
            "{message} says nothing about the cause"
        );
    }

    #[test]
    fn the_net_thread_starts_and_runs_a_queued_job() {
        // Covers the restructured bootstrap: the runtime is built inside the
        // spawned thread instead of on the caller's (GUI) thread, where the
        // two `expect`s used to be an abort a QML click could not catch.
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let tx = net().expect("the net thread must start");
        let ran = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&ran);
        tx.send(Box::new(move |_rt| {
            seen.fetch_add(1, Ordering::SeqCst);
        }))
        .expect("queued");
        for _ in 0..200 {
            if ran.load(Ordering::SeqCst) == 1 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("the net thread never ran the queued job");
    }
}
