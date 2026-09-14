//! Background network jobs. IMAP/SMTP never run on the Qt GUI thread.

use std::pin::Pin;
use std::sync::{mpsc, OnceLock};

use cxx_qt::Threading;
use cxx_qt_lib::QString;

use crate::bridge::qobject;
use crate::bridge::session::guard_sync;
use crate::bridge::{open_db, push_feeds, qstring};

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

type JobFn = Box<dyn FnOnce() + Send>;

fn net_tx() -> &'static mpsc::Sender<JobFn> {
    static TX: OnceLock<mpsc::Sender<JobFn>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<JobFn>();
        std::thread::Builder::new()
            .name("mailclient-net".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job();
                }
            })
            .expect("mailclient-net thread");
        tx
    })
}

/// Run `op` on the network thread. Returns immediately with `""` (queued) or
/// a busy message. Completion is [`qobject::Bridge::job_finished`]; `op`
/// returns `None` for the refresh when it changed nothing the feeds show, so
/// reading a draft or saving an attachment does not rebuild the message list.
pub(crate) fn spawn_job(
    mut bridge: Pin<&mut qobject::Bridge>,
    kind: &str,
    op: impl FnOnce(&mailcore::Db, &JobProgress) -> Result<(String, Option<JobRefresh>), String>
        + Send
        + 'static,
) -> QString {
    if *bridge.busy() {
        return qstring("busy — wait for the current action");
    }
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
    let _ = net_tx().send(Box::new(move || {
        let outcome = guard_sync(&kind_owned, || {
            let db = open_db()?;
            op(&db, &progress)
        });
        let (status, refresh) = match outcome {
            Ok((status, refresh)) => (status, refresh),
            Err(e) => (e, None),
        };
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
                if let Ok(db) = open_db() {
                    push_feeds(&mut bridge, &db, target.account_id, target.folder_id);
                }
            }
            bridge.as_mut().set_busy(false);
            let kind = qstring(&kind_owned);
            let status = qstring(&status);
            bridge.job_finished(&kind, &status);
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
    use super::{JobRefresh, Selection};

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
}
