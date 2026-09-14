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
/// a busy message. Completion is [`qobject::Bridge::job_finished`].
pub(crate) fn spawn_job(
    mut bridge: Pin<&mut qobject::Bridge>,
    kind: &str,
    op: impl FnOnce(&mailcore::Db) -> Result<(String, JobRefresh), String> + Send + 'static,
) -> QString {
    if *bridge.busy() {
        return qstring("busy — wait for the current action");
    }
    bridge.as_mut().set_busy(true);
    let qt = bridge.qt_thread();
    let kind_owned = kind.to_string();
    let _ = net_tx().send(Box::new(move || {
        let outcome = guard_sync(&kind_owned, || {
            let db = open_db()?;
            op(&db)
        });
        let (status, refresh) = match outcome {
            Ok((status, refresh)) => (status, Some(refresh)),
            Err(e) => (e, None),
        };
        let _ = qt.queue(move |mut bridge| {
            if let Some(refresh) = refresh {
                if let Some(limit) = refresh.message_limit {
                    bridge.as_mut().set_message_limit(limit);
                }
                if let Ok(db) = open_db() {
                    push_feeds(&mut bridge, &db, refresh.account_id, refresh.folder_id);
                }
            }
            bridge.as_mut().set_busy(false);
            let kind = qstring(&kind_owned);
            let status = qstring(&status);
            bridge.job_finished(&kind, &status);
        });
    }));
    qstring("")
}
