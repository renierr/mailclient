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

/// Move a held outbox row to a new future instant (`send_at` any RFC3339
/// instant, at least a minute out). Returns the normalized UTC instant the
/// row now waits for, so the app can re-arm its exact alarm. Local-only.
pub fn reschedule_outbox(account_id: i64, id: i64, send_at: String) -> anyhow::Result<String> {
    Ok(outbox::reschedule(shared_db()?, account_id, id, &send_at)?)
}

/// Send a `scheduled` row with the next sync instead of at its time.
/// Local-only.
pub fn send_now_outbox(account_id: i64, id: i64) -> anyhow::Result<()> {
    Ok(outbox::send_now(shared_db()?, account_id, id)?)
}

/// Earliest held `scheduled` instant of this account (UTC RFC3339), `""`
/// when nothing waits. Local read.
pub fn next_scheduled(account_id: i64) -> anyhow::Result<String> {
    Ok(outbox::next_scheduled(shared_db()?, account_id)?.unwrap_or_default())
}

/// Earliest held `scheduled` send across all accounts as
/// `{"account_id","send_at"}` (`{}` when nothing waits): what the exact
/// alarm arms from. Local read.
pub fn next_send_due() -> anyhow::Result<String> {
    let due = mailcore::store::queue::next_due(shared_db()?)?;
    Ok(match due {
        Some((account_id, send_at)) => {
            serde_json::json!({"account_id": account_id, "send_at": send_at}).to_string()
        }
        None => "{}".to_string(),
    })
}
