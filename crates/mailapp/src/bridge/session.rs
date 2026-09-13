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
    /// a two-line change. All invokables run on the Qt GUI thread, so the
    /// mutex is uncontended in practice; `into_inner` on poison keeps a
    /// panicking action from bricking later ones.
    static POOL: OnceLock<Mutex<HashMap<i64, ImapSync>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Check out the pooled session for `account`: reuse while healthy, else
/// drop it and connect fresh. The fresh connect re-reads the keyring, so a
/// changed password heals automatically on the next action.
pub(crate) fn pooled_session<'a>(
    pool: &'a mut HashMap<i64, ImapSync>,
    account: &Account,
) -> Result<&'a mut ImapSync, String> {
    let id = account.id;
    let reusable = pool.get_mut(&id).map(|s| s.is_healthy()).unwrap_or(false);
    if reusable {
        // Info, not debug: this is the line that proves the pool works
        // (one per action, same as the connecting/logged-in pair it replaces).
        log::info!("imap: reusing pooled session for account {id}");
    } else {
        if pool.remove(&id).is_some() {
            log::info!("imap: pooled session for account {id} went stale, reconnecting");
        }
        let secrets = auth::load_account_secrets(&account.auth_vault_key)
            .map_err(|e| format!("no password in keyring: {e}"))?;
        let mut fresh = ImapSync::new(account);
        fresh
            .connect(&secrets.imap_password)
            .map_err(|e| e.to_string())?;
        pool.insert(id, fresh);
    }
    Ok(pool.get_mut(&id).expect("session just pooled"))
}

/// Run `op` on the account's pooled session. Any failure evicts the session
/// so the next action reconnects fresh — a failed op may leave the stream
/// desynced, and the old connect-per-action code never reused a session
/// past one op either. Same failure guarantee, without the handshake.
pub(crate) fn with_imap<T>(
    account: &Account,
    op: impl FnOnce(&mut ImapSync) -> Result<T, String>,
) -> Result<T, String> {
    let mut pool = imap_pool();
    let session = pooled_session(&mut pool, account)?;
    let id = account.id;
    let result = op(session);
    if result.is_err() {
        pool.remove(&id);
    }
    result
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
pub(crate) fn guard_sync(
    label: &str,
    f: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
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
