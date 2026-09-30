//! Pooled IMAP sessions and panic containment for background jobs.
//!
//! Shared by every frontend (`mailapp`, `mailffi`) — and previously duplicated
//! in both. The logic is UI-free, so it lives here, next to the [`ImapSync`]
//! sessions it pools and the [`headless`](super::headless) orchestration the
//! frontends already share. There must be exactly one copy: a fix applied to
//! only one frontend's pool is the drift this module exists to prevent.
//!
//! ## Threading contract
//!
//! Sessions are checked out under a mutex but are **not** safe to drive from
//! two threads at once. Every caller runs its network jobs on a single
//! dedicated thread (the `mailclient-net` thread in both frontends), so the
//! mutex only ever serializes checkout, never concurrent use. Driving a
//! checked-out session from anywhere else is a bug, not a supported pattern.
//!
//! All locks are poison-tolerant (`into_inner` on poison): a panicking job
//! must never brick later jobs.

use std::collections::HashMap;
use std::sync::Mutex;

use super::imap::ImapSync;
use crate::auth;
use crate::db::Db;
use crate::models::Account;
use crate::store::accounts;

/// Resolve an account: the requested id if it still exists, else the first.
///
/// Lenient on purpose — selection UIs call this with "whatever was selected
/// last", which may have been deleted since. Jobs captured against a removed
/// account need [`job_account`] instead.
pub fn resolve_account(db: &Db, wanted: i64) -> Result<Account, String> {
    if wanted >= 0 {
        if let Ok(a) = accounts::get(db, wanted) {
            return Ok(a);
        }
    }
    accounts::list(db)
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "no account — add one first".to_string())
}

/// Resolve the account a background job captured when it was queued. Unlike
/// [`resolve_account`] this never substitutes another account for a captured
/// id: a send or draft meant for a removed account must fail, not land in
/// whichever account happens to be first. `-1` (nothing selected when the job
/// was queued) still means "the first account".
pub fn job_account(db: &Db, captured: i64) -> Result<Account, String> {
    if captured < 0 {
        return resolve_account(db, captured);
    }
    accounts::get(db, captured)
        .map_err(|_| "the account was removed before this action could run".to_string())
}

/// One live IMAP session per account, reused while healthy, so only the first
/// action after a drop pays TCP + TLS + LOGIN.
///
/// Process-global rather than a per-bridge field: bridge entry points cannot
/// hand out the `&mut` a checkout needs, while a module pool keeps every call
/// site a two-line change. See the module docs for the threading contract.
fn imap_pool() -> std::sync::MutexGuard<'static, Pool> {
    static POOL: std::sync::OnceLock<Mutex<Pool>> = std::sync::OnceLock::new();
    POOL.get_or_init(|| Mutex::new(Pool::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Idle sessions plus a per-account generation. Eviction bumps the
/// generation, so a session that was checked out *before* an account edit or
/// delete is disconnected on check-in instead of going back into the pool
/// with the old host and password.
#[derive(Default)]
struct Pool {
    idle: HashMap<i64, ImapSync>,
    generation: HashMap<i64, u64>,
}

impl Pool {
    fn generation(&self, account_id: i64) -> u64 {
        self.generation.get(&account_id).copied().unwrap_or(0)
    }
}

/// A checked-out session. Check it back in on clean completion; anything else
/// (error, panic, early return) disconnects on drop so a desynced stream is
/// never handed to the next checkout.
pub struct SessionLease {
    account_id: i64,
    generation: u64,
    session: Option<ImapSync>,
}

impl SessionLease {
    /// Return the session to the pool on clean completion. A cheap presence
    /// check only — it cannot await a NOOP round-trip. Stale sessions are
    /// caught at the next checkout via `is_healthy().await` instead of
    /// serving work.
    pub fn checkin(mut self) {
        if let Some(mut s) = self.session.take() {
            let mut pool = imap_pool();
            if pool.generation(self.account_id) != self.generation {
                drop(pool);
                log::info!(
                    "imap: account {} changed while leased, dropping its session",
                    self.account_id
                );
                s.disconnect();
            } else if s.is_connected() {
                pool.idle.insert(self.account_id, s);
            }
        }
    }
}

impl std::ops::Deref for SessionLease {
    type Target = ImapSync;
    fn deref(&self) -> &Self::Target {
        self.session.as_ref().expect("session present")
    }
}

impl std::ops::DerefMut for SessionLease {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.session.as_mut().expect("session present")
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        if let Some(mut s) = self.session.take() {
            log::info!(
                "imap: dropping un-checked-in session for account {}",
                self.account_id
            );
            s.disconnect();
        }
    }
}

/// Check out a pooled session: reuse while healthy, else connect fresh. The
/// fresh connect re-reads the vault, so a changed password heals itself on
/// the next action.
pub async fn checkout_session(account: &Account) -> Result<SessionLease, String> {
    let id = account.id;
    let (existing, generation) = {
        let mut pool = imap_pool();
        (pool.idle.remove(&id), pool.generation(id))
    };
    let session = match existing {
        Some(mut s) => {
            if s.is_healthy().await {
                log::debug!("imap: reusing pooled session for account {id}");
                s
            } else {
                log::info!("imap: pooled session for account {id} went stale, reconnecting");
                connect_fresh(account).await?
            }
        }
        None => connect_fresh(account).await?,
    };
    Ok(SessionLease {
        account_id: id,
        generation,
        session: Some(session),
    })
}

async fn connect_fresh(account: &Account) -> Result<ImapSync, String> {
    let secrets = auth::load_account_secrets(&account.auth_vault_key)
        .map_err(|e| format!("no password in vault: {e}"))?;
    let mut fresh = ImapSync::new(account);
    fresh
        .connect(&secrets.imap_password)
        .await
        .map_err(|e| e.to_string())?;
    Ok(fresh)
}

/// Drop one account's pooled session (account edited or deleted).
pub fn evict_session(account_id: i64) {
    let mut pool = imap_pool();
    *pool.generation.entry(account_id).or_insert(0) += 1;
    if pool.idle.remove(&account_id).is_some() {
        log::info!("imap: evicted pooled session for account {account_id}");
    }
}

/// Drop every pooled session (app shutdown). Deliberately no LOGOUT
/// round-trip: a stale connection (laptop slept, server rebooted) would block
/// the wait for BYE with no read timeout. Closing the sockets reaps the
/// server-side sessions just as well, exactly like a network drop.
pub fn drop_all_sessions() {
    let n = imap_pool().idle.drain().count();
    if n > 0 {
        log::info!("imap: dropped {n} pooled session(s) on shutdown");
    }
}

/// Run a fallible job, converting a Rust panic into an error string.
///
/// A panic escaping a job would take its thread with it, and every later
/// queued job would then be dropped on the floor — sync, send and downloads
/// silently dead until restart. (On the Qt side it is worse: a panic crossing
/// the QML bridge aborts the process.) It has to come back as a value; the
/// payload is logged for diagnosis.
pub fn guard<T>(label: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "unknown cause".to_string());
            log::error!("{label} aborted by panic: {detail}");
            Err(format!("{label} hit an internal error ({detail})"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAccount;
    use crate::store::accounts::create;

    fn sample() -> NewAccount {
        NewAccount {
            name: "Work".to_string(),
            email_address: "user@example.com".to_string(),
            from_name: String::new(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "user".to_string(),
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 465,
            smtp_security: "tls".to_string(),
            smtp_username: "user".to_string(),
            auth_vault_key: "vault:work".to_string(),
            check_interval_secs: 300,
        }
    }

    #[test]
    fn eviction_bumps_only_that_accounts_generation() {
        // One lock per statement: two guards in one expression deadlock.
        let generation_of = |id| imap_pool().generation(id);
        let (a, b) = (generation_of(9001), generation_of(9002));
        evict_session(9001);
        assert_eq!(generation_of(9001), a + 1);
        assert_eq!(generation_of(9002), b);
    }

    #[test]
    fn a_panicking_job_comes_back_as_an_error() {
        let err = guard("probe", || -> Result<(), String> { panic!("boom") }).unwrap_err();
        assert!(err.contains("probe"), "{err}");
        assert!(err.contains("boom"), "{err}");

        assert_eq!(guard("probe", || Ok::<i32, String>(7)).unwrap(), 7);
        assert_eq!(
            guard("probe", || Err::<i32, String>("plain".into())).unwrap_err(),
            "plain"
        );
    }

    #[test]
    fn resolvers_fall_back_strictly_or_not() {
        let db = Db::open_in_memory().unwrap();
        assert!(resolve_account(&db, 1).is_err());
        assert!(job_account(&db, -1).is_err());

        let id = create(&db, &sample()).unwrap();
        // Lenient: unknown and negative ids fall back to the first account.
        assert_eq!(resolve_account(&db, id + 99).unwrap().id, id);
        assert_eq!(resolve_account(&db, -1).unwrap().id, id);
        // Strict: a captured id must still exist; -1 still means "first".
        assert!(job_account(&db, id + 99).is_err());
        assert_eq!(job_account(&db, id).unwrap().id, id);
        assert_eq!(job_account(&db, -1).unwrap().id, id);
    }
}
