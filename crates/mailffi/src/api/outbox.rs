//! Outbox visibility — a thin adapter over `mailcore::outbox`, which owns
//! the counts, the rows and the dismiss rule. Retrying needs no path of its
//! own: the next sync flushes every submittable row.

use mailcore::outbox;

use crate::db::shared_db;

/// The account's unsent mail as JSON (`[{id, status, state, last_error,
/// retries, retryable, has_bytes, envelope_from, envelope_to, subject,
/// created_at, updated_at}]`). Local SQLite read, no network.
pub fn outbox_json(account_id: i64) -> anyhow::Result<String> {
    Ok(outbox::list_json(shared_db()?, account_id)?)
}

/// The account's outbox counts as JSON (`{queued, sending, failed,
/// retryable, pending}`). Local read.
pub fn outbox_status_json(account_id: i64) -> anyhow::Result<String> {
    Ok(outbox::status_json(shared_db()?, account_id)?)
}

/// Forget one queued send (a failed send the user owns the retry for, or a
/// stale entry). Local-only.
pub fn dismiss_outbox(account_id: i64, id: i64) -> anyhow::Result<()> {
    let db = shared_db()?;
    if outbox::dismiss(db, account_id, id)? == 0 {
        anyhow::bail!("message is no longer available");
    }
    Ok(())
}
