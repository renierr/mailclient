use std::collections::HashMap;
use std::sync::Mutex;

use mailcore::auth;
use mailcore::models::Account;
use mailcore::store::accounts;
use mailcore::sync::imap::ImapSync;

/// Resolve the current account: stored id if still present, else the first.
pub(crate) fn current_account(db: &mailcore::Db, wanted: i64) -> Result<Account, String> {
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

/// Connect an IMAP session using the keyring secret.
pub(crate) fn imap_pool() -> std::sync::MutexGuard<'static, HashMap<i64, ImapSync>> {
    use std::sync::OnceLock;
    /// One live IMAP session per account, reused across actions while its
    /// NOOP answers. Previously every action paid a fresh TCP + TLS + LOGIN;
    /// now only the first action (or the first after a drop) does.
    /// Process-global rather than a `BridgeRust` field: bridge invokables
    /// only expose `Pin<&mut Self>`, which cannot hand out the `&mut`
    /// HashMap a checkout needs, while a module pool keeps every call site
    /// a two-line change. Network jobs run on the mailclient-net thread, so
    /// the mutex serializes checkout; `into_inner` on poison keeps a
    /// panicking action from bricking later ones.
    static POOL: OnceLock<Mutex<HashMap<i64, ImapSync>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub(crate) struct SessionLease {
    account_id: i64,
    session: Option<ImapSync>,
}

impl SessionLease {
    /// Return the session to the pool on clean completion. This is a cheap
    /// presence check only — it cannot await a NOOP round-trip. Stale
    /// sessions are detected at the next checkout via `is_healthy().await`
    /// and dropped there instead of serving work.
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
        // If not checked in (e.g. error, panic, or early return),
        // we disconnect to ensure the stream isn't left in a desynced state.
        if let Some(mut s) = self.session.take() {
            log::info!(
                "imap: dropping un-checked-in session for account {}",
                self.account_id
            );
            s.disconnect();
        }
    }
}

/// Check out a pooled session for `account`: reuse while healthy, else
/// drop it and connect fresh. The fresh connect re-reads the keyring, so a
/// changed password heals automatically on the next action.
pub(crate) async fn checkout_session(account: &Account) -> Result<SessionLease, String> {
    let id = account.id;
    let existing = {
        let mut pool = imap_pool();
        pool.remove(&id)
    };
    let session = match existing {
        Some(mut s) => {
            if s.is_healthy().await {
                log::info!("imap: reusing pooled session for account {id}");
                s
            } else {
                log::info!("imap: pooled session for account {id} went stale, reconnecting");
                let secrets = auth::load_account_secrets(&account.auth_vault_key)
                    .map_err(|e| format!("no password in keyring: {e}"))?;
                let mut fresh = ImapSync::new(account);
                fresh
                    .connect(&secrets.imap_password)
                    .await
                    .map_err(|e| e.to_string())?;
                fresh
            }
        }
        None => {
            let secrets = auth::load_account_secrets(&account.auth_vault_key)
                .map_err(|e| format!("no password in keyring: {e}"))?;
            let mut fresh = ImapSync::new(account);
            fresh
                .connect(&secrets.imap_password)
                .await
                .map_err(|e| e.to_string())?;
            fresh
        }
    };
    Ok(SessionLease {
        account_id: id,
        session: Some(session),
    })
}

/// Drop one account's pooled session (account edited or deleted).
pub(crate) fn evict_imap_session(account_id: i64) {
    if imap_pool().remove(&account_id).is_some() {
        log::info!("imap: evicted pooled session for account {account_id}");
    }
}

/// Drop every pooled session (app quit). Deliberately no LOGOUT round-trip:
/// a stale pooled connection (laptop slept, server rebooted) would block
/// quit on the BYE wait with no read timeout — closing the sockets reaps
/// the server-side sessions just as well, exactly like a network drop.
pub(crate) fn drop_all_imap_sessions() {
    let n = imap_pool().drain().count();
    if n > 0 {
        log::info!("imap: dropped {n} pooled session(s) on quit");
    }
}

/// Run a fallible sync action, converting a Rust panic into an error string.
///
/// cxx turns any panic crossing the QML bridge into SIGABRT (its Guard
/// double-panics by design), which kills the app on something as routine as
/// startup auto-sync. A sync panic must surface as a status message instead —
/// the failure is logged with its payload for diagnosis.
pub(crate) fn guard_sync<T>(
    label: &str,
    f: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
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
    use super::guard_sync;

    /// The net thread runs every job through this (see `worker::net_tx`), so
    /// a panic has to come back as a value — unwinding past it would take the
    /// thread down and silently strand all later background work.
    #[test]
    fn a_panicking_job_comes_back_as_an_error() {
        let err = guard_sync("probe", || -> Result<(), String> {
            panic!("boom");
        })
        .unwrap_err();
        assert!(err.contains("probe"), "{err}");
        assert!(err.contains("boom"), "{err}");

        assert_eq!(guard_sync("probe", || Ok::<i32, String>(7)).unwrap(), 7);
        assert_eq!(
            guard_sync("probe", || Err::<i32, String>("plain".into())).unwrap_err(),
            "plain"
        );
    }
}
