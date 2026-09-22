//! IMAP session pooling and panic containment for background jobs.
//!
//! A near-copy of `mailapp::bridge::session` — the logic is Qt-free, it just
//! lives on the wrong side of the Qt boundary today. Once both frontends are
//! real this should be promoted into `mailcore::sync` and deleted from both
//! crates; see `flutter/README.md` ("Shared code still to promote").

use std::collections::HashMap;
use std::sync::Mutex;

use mailcore::auth;
use mailcore::models::Account;
use mailcore::store::accounts;
use mailcore::sync::imap::ImapSync;

/// Resolve an account: the requested id if it still exists, else the first.
pub(crate) fn resolve_account(db: &mailcore::Db, wanted: i64) -> Result<Account, String> {
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

/// One live IMAP session per account, reused while its NOOP answers, so only
/// the first action after a drop pays TCP + TLS + LOGIN. Network jobs all run
/// on the `mailclient-net` thread, so the mutex only ever serializes checkout;
/// `into_inner` on poison keeps a panicking action from bricking later ones.
fn imap_pool() -> std::sync::MutexGuard<'static, HashMap<i64, ImapSync>> {
    static POOL: std::sync::OnceLock<Mutex<HashMap<i64, ImapSync>>> = std::sync::OnceLock::new();
    POOL.get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub(crate) struct SessionLease {
    account_id: i64,
    session: Option<ImapSync>,
}

impl SessionLease {
    /// Return the session to the pool on clean completion. A cheap presence
    /// check only — it cannot await a NOOP round-trip. Stale sessions are
    /// caught at the next checkout instead of serving work.
    pub fn checkin(mut self) {
        if let Some(s) = self.session.take() {
            if s.is_connected() {
                imap_pool().insert(self.account_id, s);
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
        // Not checked in (error, panic, early return): disconnect, or the
        // stream is left desynced for whoever picks the session up next.
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
/// fresh connect re-reads the keyring, so a changed password heals itself.
pub(crate) async fn checkout_session(account: &Account) -> Result<SessionLease, String> {
    let id = account.id;
    let existing = imap_pool().remove(&id);
    let session = match existing {
        Some(mut s) => {
            if s.is_healthy().await {
                log::info!("imap: reusing pooled session for account {id}");
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
        session: Some(session),
    })
}

async fn connect_fresh(account: &Account) -> Result<ImapSync, String> {
    let secrets = auth::load_account_secrets(&account.auth_vault_key)
        .map_err(|e| format!("no password in keyring: {e}"))?;
    let mut fresh = ImapSync::new(account);
    fresh
        .connect(&secrets.imap_password)
        .await
        .map_err(|e| e.to_string())?;
    Ok(fresh)
}

/// Drop one account's pooled session (account edited or deleted).
pub(crate) fn evict_session(account_id: i64) {
    if imap_pool().remove(&account_id).is_some() {
        log::info!("imap: evicted pooled session for account {account_id}");
    }
}

/// Drop every pooled session (app shutdown). Deliberately no LOGOUT
/// round-trip: a stale connection (laptop slept, server rebooted) would block
/// the wait for BYE with no read timeout. Closing the sockets reaps the
/// server-side sessions just as well, exactly like a network drop.
pub(crate) fn drop_all_sessions() {
    let n = imap_pool().drain().count();
    if n > 0 {
        log::info!("imap: dropped {n} pooled session(s) on shutdown");
    }
}

/// Run a fallible job, converting a Rust panic into an error string.
///
/// A panic unwinding out of a job would take the `mailclient-net` thread with
/// it, and every later queued job would then be dropped on the floor — sync,
/// send and downloads silently dead until restart. It has to come back as a
/// value; the payload is logged for diagnosis.
pub(crate) fn guard<T>(label: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
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
    use super::guard;

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
}
