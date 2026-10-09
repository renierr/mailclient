//! "Push once the Undo grace period is over", for every undoable action.
//!
//! Both adapters used to start one sleeping OS thread per action, so N
//! deletes in a row meant N threads and N queued pushes (D8, E10). This is
//! one timer thread for the process with a queue of deadlines, and one
//! account's actions that fall due close together are pushed once: the
//! push sends everything due for the account anyway.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Deadlines this close behind one being fired are fired with it.
const COALESCE: Duration = Duration::from_secs(2);

/// What to run for an account once it is due: the adapter's flag push.
pub type Due = fn(i64);

/// Pending `(deadline, account)`, earliest first.
type Deadlines = BinaryHeap<Reverse<(Instant, i64)>>;

/// A timer thread with a queue of `(deadline, account)`. Cloning shares it.
#[derive(Clone)]
pub(crate) struct GraceTimer {
    inner: Arc<(Mutex<Deadlines>, Condvar)>,
    delay: Duration,
}

impl GraceTimer {
    /// Start the thread. `delay` is how long after [`Self::schedule`] an
    /// account is due.
    pub(crate) fn start(delay: Duration, due: Due) -> std::io::Result<Self> {
        let timer = Self {
            inner: Arc::new((Mutex::new(BinaryHeap::new()), Condvar::new())),
            delay,
        };
        let worker = timer.clone();
        std::thread::Builder::new()
            .name("mailclient-undo".into())
            .spawn(move || worker.run(due))?;
        Ok(timer)
    }

    pub(crate) fn schedule(&self, account_id: i64) {
        let (queue, wake) = &*self.inner;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.push(Reverse((Instant::now() + self.delay, account_id)));
        wake.notify_one();
    }

    fn run(&self, due: Due) {
        let (queue, wake) = &*self.inner;
        let mut queue = queue.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let Some(&Reverse((at, account))) = queue.peek() else {
                queue = wake.wait(queue).unwrap_or_else(|e| e.into_inner());
                continue;
            };
            let now = Instant::now();
            if at > now {
                queue = wake
                    .wait_timeout(queue, at - now)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
                continue;
            }
            queue.pop();
            // Later actions of the same account that are about due ride along.
            let mut rest = Vec::new();
            while let Some(Reverse((later, other))) = queue.pop() {
                if other == account && later <= at + COALESCE {
                    continue;
                }
                rest.push(Reverse((later, other)));
                if later > at + COALESCE {
                    break;
                }
            }
            queue.extend(rest);
            drop(queue);
            due(account);
            queue = self.inner.0.lock().unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// Run `due(account_id)` once [`super::UNDO_GRACE_SECS`] (plus a second) have
/// passed. If the process quits first, the next sync pushes the move
/// instead. Without a timer thread (the OS refused one) the action stays
/// queued for that sync too.
///
/// The timer is per process and keeps the `due` of its first call: each
/// process hosts one adapter, which always passes its own flag push.
pub fn push_after_grace(account_id: i64, due: Due) {
    static TIMER: OnceLock<Option<GraceTimer>> = OnceLock::new();
    let timer = TIMER.get_or_init(|| {
        let delay = Duration::from_secs(super::UNDO_GRACE_SECS.max(0) as u64 + 1);
        GraceTimer::start(delay, due)
            .inspect_err(|e| {
                log::warn!("undo: no timer thread, pushes wait for the next sync: {e}")
            })
            .ok()
    });
    if let Some(timer) = timer {
        timer.schedule(account_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    static CALLS: StdMutex<Vec<i64>> = StdMutex::new(Vec::new());

    fn record(account: i64) {
        CALLS.lock().unwrap().push(account);
    }

    #[test]
    fn one_thread_coalesces_an_accounts_burst() {
        let timer = GraceTimer::start(Duration::from_millis(40), record).unwrap();
        for _ in 0..5 {
            timer.schedule(1);
        }
        timer.schedule(2);
        std::thread::sleep(Duration::from_millis(400));
        let mut calls = CALLS.lock().unwrap().clone();
        calls.sort_unstable();
        assert_eq!(calls, vec![1, 2], "five actions of account 1 push once");
    }
}
