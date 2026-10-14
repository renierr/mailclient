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
    Ok(outbox::dismiss(shared_db()?, account_id, id)?)
}

/// Move a waiting outbox row to a new future instant (`send_at` any RFC3339
/// instant, at least a minute out). Returns the normalized UTC instant the
/// row now waits for, so the app can re-arm its exact alarm. Local-only.
pub fn reschedule_outbox(account_id: i64, id: i64, send_at: String) -> anyhow::Result<String> {
    Ok(outbox::reschedule(shared_db()?, account_id, id, &send_at)?)
}

/// Send a waiting scheduled row now instead of at its time (due at once for
/// the alarm). Local-only.
pub fn send_now_outbox(account_id: i64, id: i64) -> anyhow::Result<()> {
    Ok(outbox::send_now(shared_db()?, account_id, id)?)
}

/// Flush every due scheduled send, whatever account holds it (no poll
/// gating: intervals, quiet hours and push/poll mode never hold back mail
/// the user asked to send). Blocks for the network run, so only a worker
/// thread calls it. Returns `{sent, skipped, errors, run}` as JSON.
pub fn send_due() -> anyhow::Result<String> {
    let report = mailcore::sync::background::send_due_scheduled_blocking(
        shared_db()?,
        &crate::db::db_path(),
    );
    Ok(serde_json::to_string(&report)?)
}
