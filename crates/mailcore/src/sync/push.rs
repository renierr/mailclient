//! Push mail: one IMAP connection per account parked in IDLE on the inbox,
//! so the server announces new mail instead of the phone polling for it.
//!
//! [`PushMonitor`] owns a dedicated `mailclient-push` thread with a
//! current-thread Tokio runtime; every account is a local task on it, over
//! one SQLite connection. When the server reports a change, the task ends
//! the IDLE, runs [`background::push_check`] over the same session (no
//! reconnect, no new TLS handshake) and hands the report to the
//! [`PushListener`], which posts the notification.
//!
//! The monitor never keeps time itself while the phone sleeps: Tokio's
//! timers stand still in suspend. The host sends [`PushMonitor::keepalive`]
//! from a wake-up alarm instead, which re-issues every IDLE (keeping the
//! connection and any NAT mapping alive) and retries accounts that are
//! backing off after an error. [`PushMonitor::network_changed`] reconnects
//! everything at once when the default network changes.
//!
//! [`PushListener::busy`] brackets every stretch of work, so the host holds
//! a wake lock exactly while the monitor needs the CPU and not while it
//! waits.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::auth;
use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::Account;
use crate::store::{account_settings, accounts};
use crate::sync::background::{self, schedule, BackgroundReport};
use crate::sync::imap::{IdleEnd, ImapSync};

/// Re-issue IDLE at least this often while the phone is awake. RFC 2177
/// lets a server drop an IDLE after 30 minutes; while the phone sleeps the
/// host's keep-alive alarm takes this over.
pub const IDLE_REFRESH: Duration = Duration::from_secs(25 * 60);

/// What the monitor tells its host. Called on the push thread.
pub trait PushListener: Send + Sync + 'static {
    /// `true` when work starts, `false` once every account is back to
    /// waiting (in IDLE, backing off, or offline).
    fn busy(&self, busy: bool);

    /// A check finished with something to report. The listener posts or
    /// updates the notification and commits the report's marks before it
    /// returns, so the next check never reports the same mail again.
    fn report(&self, report: &BackgroundReport);
}

/// What the host last asked for. Tasks compare snapshots: any change ends
/// the current wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Signal {
    keepalive: u64,
    reconnect: u64,
    online: bool,
    stop: bool,
}

/// Handle to the running monitor. Dropping it stops the monitor without
/// waiting for the thread: the accounts log out on their own.
pub struct PushMonitor {
    signal: watch::Sender<Signal>,
}

impl PushMonitor {
    /// Start watching every push account in the database at `db_path`
    /// ([`schedule::push_account_ids`]). Accounts added, or switched to or
    /// from push, later are picked up on the next keep-alive or network
    /// change.
    pub fn start(db_path: PathBuf, listener: Arc<dyn PushListener>, online: bool) -> Result<Self> {
        let (signal, rx) = watch::channel(Signal {
            online,
            ..Default::default()
        });
        std::thread::Builder::new()
            .name("mailclient-push".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        log::error!("push: no runtime: {e}");
                        return;
                    }
                };
                tokio::task::LocalSet::new().block_on(&rt, supervise(db_path, listener, rx));
                log::info!("push: monitor stopped");
            })
            .map_err(|e| StoreError::InvalidInput(format!("push thread: {e}")))?;
        Ok(Self { signal })
    }

    /// Refresh every IDLE now and retry accounts that are backing off.
    pub fn keepalive(&self) {
        self.signal.send_modify(|s| s.keepalive += 1);
    }

    /// The default network changed (or went away): reconnect every account
    /// once `online` is true.
    pub fn network_changed(&self, online: bool) {
        self.signal.send_modify(|s| {
            s.online = online;
            s.reconnect += 1;
        });
    }

    /// Stop all accounts; they end their IDLE and log out.
    pub fn stop(&self) {
        self.signal.send_modify(|s| s.stop = true);
    }
}

impl Drop for PushMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Ctx {
    db: Db,
    db_path: PathBuf,
    listener: Arc<dyn PushListener>,
    busy: Cell<usize>,
}

/// Keeps one task per push account and re-reads the account list on every
/// signal. An account that left push is dropped mid-IDLE: the server sees
/// the connection close, which is all a LOGOUT would have told it.
async fn supervise(
    db_path: PathBuf,
    listener: Arc<dyn PushListener>,
    mut rx: watch::Receiver<Signal>,
) {
    let db = match Db::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            log::error!("push: database: {e}");
            return;
        }
    };
    let ctx = Rc::new(Ctx {
        db,
        db_path,
        listener,
        busy: Cell::new(0),
    });
    let mut tasks: HashMap<i64, tokio::task::JoinHandle<()>> = HashMap::new();
    loop {
        if rx.borrow_and_update().stop {
            break;
        }
        let wanted = schedule::push_account_ids(&ctx.db);
        tasks.retain(|id, task| {
            let keep = wanted.contains(id) && !task.is_finished();
            if !keep {
                task.abort();
            }
            keep
        });
        for id in wanted {
            tasks.entry(id).or_insert_with(|| {
                tokio::task::spawn_local(run_account(ctx.clone(), id, rx.clone()))
            });
        }
        if rx.changed().await.is_err() {
            break;
        }
    }
    for (_, task) in tasks {
        let _ = task.await;
    }
}

/// Counts this task into the monitor's busy total while `held`.
struct Busy {
    ctx: Rc<Ctx>,
    held: bool,
}

impl Busy {
    fn new(ctx: Rc<Ctx>) -> Self {
        let mut busy = Self { ctx, held: false };
        busy.set(true);
        busy
    }

    fn set(&mut self, on: bool) {
        if on == self.held {
            return;
        }
        self.held = on;
        let before = self.ctx.busy.get();
        let after = if on { before + 1 } else { before - 1 };
        self.ctx.busy.set(after);
        if before == 0 || after == 0 {
            self.ctx.listener.busy(after > 0);
        }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.set(false);
    }
}

/// How a session ended without an error.
enum Exit {
    Stop,
    Reconnect,
}

/// One account, for the life of the monitor: connect, IDLE, check on every
/// change, reconnect with backoff after errors. Ends when the monitor stops
/// or the account is deleted.
async fn run_account(ctx: Rc<Ctx>, account_id: i64, mut rx: watch::Receiver<Signal>) {
    let mut busy = Busy::new(ctx.clone());
    let mut failures = 0u32;
    let mut last_error: Option<String> = None;
    loop {
        let signal = *rx.borrow_and_update();
        if signal.stop {
            return;
        }
        if !signal.online {
            busy.set(false);
            if rx.changed().await.is_err() {
                return;
            }
            busy.set(true);
            continue;
        }
        let Ok(account) = accounts::get(&ctx.db, account_id) else {
            return;
        };
        match serve(&ctx, &account, &mut rx, &mut busy, &mut failures).await {
            Ok(Exit::Stop) => return,
            Ok(Exit::Reconnect) => {}
            Err(e) => {
                failures += 1;
                let message = format!("{}: {e}", account.email_address);
                log::warn!("push: {message}");
                // One history entry per distinct failure, not one per retry.
                if last_error.as_deref() != Some(message.as_str()) {
                    background::record_failed_run(&ctx.db, "push", &message);
                    last_error = Some(message);
                }
                busy.set(false);
                // Any signal retries at once: while the phone sleeps this
                // timer stands still and the keep-alive alarm wakes us.
                tokio::select! {
                    () = tokio::time::sleep(backoff(failures)) => {}
                    changed = rx.changed() => if changed.is_err() { return },
                }
                busy.set(true);
            }
        }
        if failures == 0 {
            last_error = None;
        }
    }
}

/// Connect `account`, then serve it until it errors, stops or must reconnect.
async fn serve(
    ctx: &Ctx,
    account: &Account,
    rx: &mut watch::Receiver<Signal>,
    busy: &mut Busy,
    failures: &mut u32,
) -> Result<Exit> {
    let secrets = auth::load_account_secrets_retry(&account.auth_vault_key).await?;
    let mut imap = ImapSync::new(account);
    imap.connect(&secrets.imap_password).await?;
    let result = serve_session(ctx, account, &mut imap, rx, busy, failures).await;
    match result {
        Ok(_) => imap.logout().await,
        Err(_) => imap.disconnect(),
    }
    result
}

async fn serve_session(
    ctx: &Ctx,
    account: &Account,
    imap: &mut ImapSync,
    rx: &mut watch::Receiver<Signal>,
    busy: &mut Busy,
    failures: &mut u32,
) -> Result<Exit> {
    let idle = imap.session()?.has_capability("IDLE");
    if !idle {
        log::info!("push: server has no IDLE, checking on every keep-alive");
    }
    // Catch up on whatever arrived while this account was not connected.
    check(ctx, account, imap).await?;
    *failures = 0;
    loop {
        let base = *rx.borrow_and_update();
        if let Some(exit) = exit_for(base, base) {
            return Ok(exit);
        }
        busy.set(false);
        let end = if idle {
            imap.session()?.idle(wait_for_signal(rx, base)).await
        } else {
            wait_for_signal(rx, base).await;
            Ok(IdleEnd::Changed)
        };
        busy.set(true);
        let end = end?;
        if idle {
            let stats = imap.session()?.last_idle();
            account_settings::record_idle_heartbeats(
                &ctx.db,
                account.id,
                stats.heartbeats,
                stats.heartbeat_every.map(|d| d.as_secs() as i64),
                stats.idled.as_secs() as i64,
            );
        }
        if rx.has_changed().is_err() {
            return Ok(Exit::Stop);
        }
        if let Some(exit) = exit_for(base, *rx.borrow()) {
            return Ok(exit);
        }
        // A keep-alive needs nothing more: DONE and the tagged OK already
        // went over the wire. Without IDLE the keep-alive is the check.
        if end == IdleEnd::Changed {
            check(ctx, account, imap).await?;
        }
    }
}

/// Sync the inbox over the open session and deliver the report. A report
/// with errors may just be a failed SMTP flush, so the session only counts
/// as broken when it no longer answers a NOOP.
async fn check(ctx: &Ctx, account: &Account, imap: &mut ImapSync) -> Result<()> {
    let report = background::push_check(&ctx.db, &ctx.db_path, account, imap).await;
    if !report.skipped {
        ctx.listener.report(&report);
    }
    if !report.errors.is_empty() && !imap.is_healthy().await {
        return Err(StoreError::Network(report.errors.join("; ")));
    }
    Ok(())
}

/// Whether the change from `base` to `now` ends the session.
fn exit_for(base: Signal, now: Signal) -> Option<Exit> {
    if now.stop {
        Some(Exit::Stop)
    } else if !now.online || now.reconnect != base.reconnect {
        Some(Exit::Reconnect)
    } else {
        None
    }
}

/// Resolves on the first signal that differs from `base`, when the monitor
/// goes away, or after [`IDLE_REFRESH`] of awake time.
async fn wait_for_signal(rx: &mut watch::Receiver<Signal>, base: Signal) {
    let _ = tokio::time::timeout(IDLE_REFRESH, async {
        while rx.changed().await.is_ok() {
            if *rx.borrow() != base {
                return;
            }
        }
    })
    .await;
}

/// Wait before retry `failures` (1-based): 30 s doubling up to 15 minutes,
/// the keep-alive cadence, so a dead server costs one attempt per alarm.
fn backoff(failures: u32) -> Duration {
    let secs = 30u64.saturating_mul(1 << failures.saturating_sub(1).min(5));
    Duration::from_secs(secs.min(15 * 60))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn backoff_doubles_up_to_the_keepalive_cadence() {
        assert_eq!(backoff(1), Duration::from_secs(30));
        assert_eq!(backoff(2), Duration::from_secs(60));
        assert_eq!(backoff(4), Duration::from_secs(240));
        assert_eq!(backoff(6), Duration::from_secs(900));
        assert_eq!(backoff(40), Duration::from_secs(900));
    }

    #[test]
    fn only_stop_and_network_changes_end_a_session() {
        let base = Signal {
            online: true,
            ..Default::default()
        };
        let keepalive = Signal {
            keepalive: 1,
            ..base
        };
        assert!(exit_for(base, keepalive).is_none());
        assert!(matches!(
            exit_for(
                base,
                Signal {
                    reconnect: 1,
                    ..base
                }
            ),
            Some(Exit::Reconnect)
        ));
        assert!(matches!(
            exit_for(
                base,
                Signal {
                    online: false,
                    ..base
                }
            ),
            Some(Exit::Reconnect)
        ));
        assert!(matches!(
            exit_for(base, Signal { stop: true, ..base }),
            Some(Exit::Stop)
        ));
    }

    #[derive(Default)]
    struct Recorder {
        busy: Mutex<Vec<bool>>,
    }

    impl PushListener for Recorder {
        fn busy(&self, busy: bool) {
            self.busy.lock().unwrap().push(busy);
        }
        fn report(&self, _: &BackgroundReport) {}
    }

    #[test]
    fn busy_reports_only_the_first_start_and_the_last_finish() {
        let recorder = Arc::new(Recorder::default());
        let ctx = Rc::new(Ctx {
            db: Db::open_in_memory().unwrap(),
            db_path: PathBuf::new(),
            listener: recorder.clone(),
            busy: Cell::new(0),
        });
        let mut a = Busy::new(ctx.clone());
        let mut b = Busy::new(ctx.clone());
        a.set(false);
        a.set(false);
        b.set(false);
        b.set(true);
        drop(b);
        drop(a);
        assert_eq!(
            *recorder.busy.lock().unwrap(),
            vec![true, false, true, false]
        );
        assert_eq!(ctx.busy.get(), 0);
    }

    #[tokio::test]
    async fn a_keepalive_ends_the_wait_but_an_unchanged_signal_does_not() {
        let (tx, mut rx) = watch::channel(Signal::default());
        let base = *rx.borrow_and_update();
        tx.send_modify(|s| s.keepalive += 1);
        tokio::time::timeout(Duration::from_secs(1), wait_for_signal(&mut rx, base))
            .await
            .expect("keep-alive wakes the wait");

        let base = *rx.borrow_and_update();
        tx.send_modify(|_| {});
        let waited =
            tokio::time::timeout(Duration::from_millis(50), wait_for_signal(&mut rx, base)).await;
        assert!(waited.is_err(), "a no-op send keeps waiting");
    }
}
