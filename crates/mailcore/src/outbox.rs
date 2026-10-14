//! Outbox visibility: what is still queued, sending or failed, shared by both
//! frontends.
//!
//! The send rules live in [`crate::compose`] and [`crate::sync::sender`];
//! this module only reads: status counts and list rows for the outbox dialog,
//! plus dismissing a dead row. Retrying needs no new path —
//! [`crate::sync::sender::SmtpSender::flush_outbox`] already picks up every
//! submittable row on the next sync, so "Retry now" is just a sync.

use serde_json::json;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::QueueStatus;
use crate::store::queue;

/// Per-account outbox counts for the status pill. `pending` is the pill's
/// number (`queued + sending + failed + scheduled`); `failed` drives the
/// error styling; `retryable` (bytes present, retries left, due) says whether
/// a sync could still deliver something; `scheduled` waits for its time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct OutboxStatus {
    pub queued: u64,
    pub sending: u64,
    pub failed: u64,
    pub scheduled: u64,
    pub retryable: u64,
    pub pending: u64,
}

impl OutboxStatus {
    /// Whether the pill shows at all.
    #[must_use]
    pub fn any(self) -> bool {
        self.pending > 0
    }

    /// The pill's words, phrased once for both frontends: `2 unsent`,
    /// `2 unsent (1 failed)` or `2 unsent (1 scheduled)`.
    #[must_use]
    pub fn label(self) -> String {
        let mut parts = Vec::new();
        if self.failed > 0 {
            parts.push(format!("{} failed", self.failed));
        }
        if self.scheduled > 0 {
            parts.push(format!("{} scheduled", self.scheduled));
        }
        if parts.is_empty() {
            format!("{} unsent", self.pending)
        } else {
            format!("{} unsent ({})", self.pending, parts.join(", "))
        }
    }
}

/// Count the account's outbox rows by status. Rows that can never submit
/// (queued without MIME bytes) count under `failed`.
pub fn status(db: &Db, account_id: i64) -> Result<OutboxStatus> {
    let mut queued = 0u64;
    let mut sending = 0u64;
    let mut failed = 0u64;
    let mut scheduled = 0u64;
    {
        let mut stmt = db.conn().prepare(
            "select case
                      when status in ('queued', 'scheduled')
                           and (raw_mime is null or length(raw_mime) = 0) then 'failed'
                      else status
                    end,
                    count(*)
                from send_queue
               where account_id = ?1 and status in ('queued', 'sending', 'failed', 'scheduled')
               group by 1",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })?;
        for row in rows {
            let (s, n) = row?;
            match QueueStatus::parse_status(&s) {
                QueueStatus::Queued => queued = n,
                QueueStatus::Sending => sending = n,
                QueueStatus::Failed => failed = n,
                QueueStatus::Scheduled => scheduled = n,
                QueueStatus::Sent => {}
            }
        }
    }
    // Same conditions as `queue::list_submittable`, without loading the
    // MIME bytes of every queued message. `now` comes from Rust in the same
    // shape the column stores (SQLite's `datetime('now')` writes a different
    // format that would compare wrong).
    let retryable = db.conn().query_row(
        "select count(*) from send_queue
          where account_id = ?1
            and status in ('queued', 'failed')
            and raw_mime is not null and length(raw_mime) > 0
            and retries < ?2
            and (send_at is null or send_at <= ?3)",
        rusqlite::params![
            account_id,
            queue::MAX_SEND_RETRIES as i64,
            crate::store::now()
        ],
        |r| r.get::<_, i64>(0),
    )? as u64;
    Ok(OutboxStatus {
        queued,
        sending,
        failed,
        scheduled,
        retryable,
        pending: queued + sending + failed + scheduled,
    })
}

/// `status` as JSON for the bridges (same transport as [`crate::feed`]),
/// plus what the pill shows: `label` and `has_failures` (danger styling).
pub fn status_json(db: &Db, account_id: i64) -> Result<String> {
    let s = status(db, account_id)?;
    let mut v = serde_json::to_value(s)?;
    v["label"] = json!(s.label());
    v["has_failures"] = json!(s.failed > 0);
    Ok(serde_json::to_string(&v)?)
}

/// One outbox row for the dialog: everything the UI shows, never the MIME
/// bytes (like [`crate::feed`] keeps bodies out of list rows). `state` is
/// the row's one-line state, phrased once here so both frontends show the
/// same words (like [`undo`]'s toast labels). Rows still `scheduled` carry
/// `send_at` (UTC RFC3339; `""` otherwise, also once a flush took the row)
/// for the picker's local display, and `notify_self` when delivery mails a
/// receipt to the own address. `retryable` is what a sync could deliver now,
/// as in [`status`]: never a row that waits for its time.
pub fn list_json(db: &Db, account_id: i64) -> Result<String> {
    let mut stmt = db.conn().prepare(
        "select id, status, last_error, retries, raw_mime,
                envelope_from, envelope_to, created_at, updated_at, send_at, notify_self
          from send_queue
          where account_id = ?1 and status in ('queued', 'sending', 'failed', 'scheduled')
          order by created_at",
    )?;
    let mut arr = Vec::new();
    let rows = stmt.query_map([account_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, Option<Vec<u8>>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, Option<String>>(9)?,
            r.get::<_, bool>(10)?,
        ))
    })?;
    for row in rows {
        let (
            id,
            status_raw,
            last_error,
            retries,
            raw,
            from,
            to_raw,
            created,
            updated,
            send_at,
            notify_self,
        ) = row?;
        let status = QueueStatus::parse_status(&status_raw);
        let has_bytes = raw.as_deref().is_some_and(|b| !b.is_empty());
        // `dismiss`'s rule: never a row being sent.
        let dismissable = matches!(
            status,
            QueueStatus::Queued | QueueStatus::Failed | QueueStatus::Scheduled
        );
        let retryable = matches!(status, QueueStatus::Queued | QueueStatus::Failed)
            && has_bytes
            && (retries as u64) < queue::MAX_SEND_RETRIES;
        let send_at = match status {
            QueueStatus::Scheduled => send_at.unwrap_or_default(),
            _ => String::new(),
        };
        let to: Vec<String> = serde_json::from_str(&to_raw).unwrap_or_default();
        arr.push(json!({
            "id": id,
            "status": status.as_str(),
            "state": state_line(status, retryable, has_bytes),
            "last_error": last_error.unwrap_or_default(),
            "retries": retries,
            "retryable": retryable,
            "dismissable": dismissable,
            "has_bytes": has_bytes,
            "envelope_from": from.unwrap_or_default(),
            "envelope_to": to,
            "subject": raw
                .as_deref()
                .and_then(subject_from_mime)
                .unwrap_or_else(|| "(no subject)".to_string()),
            "created_at": created,
            "updated_at": updated,
            "send_at": send_at,
            "notify_self": notify_self,
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

/// Forget one outbox row of this account (a failed send the user owns the
/// retry for, a scheduled send, or a stale entry). Fails with `NotFound` when
/// the id is gone, belongs to another account, or is `sending` right now —
/// deleting that would not stop the mail, only hide that it went out. `sent`
/// rows are [`queue::prune_sent`]'s job.
pub fn dismiss(db: &Db, account_id: i64, id: i64) -> Result<()> {
    let n = db.conn().execute(
        "delete from send_queue
          where id = ?1 and account_id = ?2 and status in ('queued', 'failed', 'scheduled')",
        rusqlite::params![id, account_id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound(
            "outbox entry (already sent, being sent, or gone)".into(),
        ));
    }
    Ok(())
}

/// Move a waiting row to a new future instant (outbox dialog's reschedule).
/// Returns the normalized UTC instant the row now waits for.
pub fn reschedule(db: &Db, account_id: i64, id: i64, send_at: &str) -> Result<String> {
    queue::reschedule(db, account_id, id, send_at)
}

/// Send a waiting scheduled row now instead of at its time.
pub fn send_now(db: &Db, account_id: i64, id: i64) -> Result<()> {
    queue::send_now(db, account_id, id)
}

/// Whether `raw` (any RFC3339 instant) can be a scheduled send's time:
/// `None` when it can, else the reason in the words the pickers show. Both
/// frontends ask this instead of repeating the lead-time rule; storing the
/// row checks again, so a dialog left open past its time is still refused.
#[must_use]
pub fn send_at_error(raw: &str) -> Option<String> {
    queue::normalize_future_send_at(raw).err().map(|e| match e {
        StoreError::InvalidInput(msg) => msg,
        other => other.to_string(),
    })
}

/// Milliseconds until the earliest waiting scheduled send wants a flush
/// across all accounts (`-1` = none waits, `0` = due now). Due timers and
/// exact alarms arm from this; a send that failed after its time comes back
/// after [`queue::SCHEDULED_RETRY_SECS`], not at once.
#[must_use]
pub fn due_in_ms(db: &Db) -> i64 {
    match queue::next_due(db) {
        Ok(Some(when)) => (when - chrono::Utc::now()).num_milliseconds().max(0),
        Ok(None) => -1,
        Err(e) => {
            log::warn!("outbox: cannot plan scheduled sends: {e}");
            -1
        }
    }
}

/// A row's one-line state from its status, whether a sync could still
/// deliver it (bytes present, retries left), and whether raw MIME bytes exist.
#[must_use]
pub fn state_line(status: QueueStatus, retryable: bool, has_bytes: bool) -> &'static str {
    match status {
        QueueStatus::Sending => "Sending…",
        QueueStatus::Scheduled => "Scheduled — sends automatically",
        QueueStatus::Queued if has_bytes => "Queued — goes out with the next sync",
        QueueStatus::Queued => "Incomplete — message data was discarded; dismiss this entry",
        QueueStatus::Failed if retryable => "Failed — will retry on the next sync",
        QueueStatus::Failed if has_bytes => {
            "Failed — retry limit reached, dismiss or resend by hand"
        }
        QueueStatus::Failed => "Failed — send it again by hand, then dismiss this entry",
        QueueStatus::Sent => "Sent",
    }
}

/// The decoded `Subject:` of a built MIME message (RFC 2047 encoded words
/// included, so an umlaut subject reads as written), `None` when absent or
/// empty. Headers only: the body is never parsed.
pub(crate) fn subject_from_mime(raw: &[u8]) -> Option<String> {
    mail_parser::MessageParser::default()
        .parse_headers(raw)?
        .subject()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests;
