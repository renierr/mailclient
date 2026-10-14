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
            and (send_at is null or send_at <= ?3)
            and (next_attempt_at is null or next_attempt_at <= ?3)",
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

/// What an outbox row is, from the user's side: the row model both
/// frontends render (badge, words, which actions to offer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A submit owns it right now: nothing to do but wait.
    Sending,
    /// Held for its time.
    Scheduled,
    /// Goes out with the next flush.
    Queued,
    /// Failed, tries again by itself (after a pause, for a scheduled send).
    Retrying,
    /// Failed for good: out of attempts, refused, or without its bytes.
    Failed,
}

impl RowKind {
    #[must_use]
    pub fn of(status: QueueStatus, has_bytes: bool, retries: u64) -> Self {
        match status {
            QueueStatus::Sending | QueueStatus::Sent => Self::Sending,
            QueueStatus::Scheduled => Self::Scheduled,
            QueueStatus::Queued => Self::Queued,
            QueueStatus::Failed if has_bytes && retries < queue::MAX_SEND_RETRIES => Self::Retrying,
            QueueStatus::Failed => Self::Failed,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sending => "sending",
            Self::Scheduled => "scheduled",
            Self::Queued => "queued",
            Self::Retrying => "retrying",
            Self::Failed => "failed",
        }
    }
}

/// What the user may do with a row, decided once for both frontends. Every
/// action needs the bytes; none touches a row being sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct RowActions {
    /// Submit now instead of waiting ([`send_now`]); starts the attempts over.
    pub can_send_now: bool,
    /// Pick a new time ([`reschedule`]).
    pub can_reschedule: bool,
    /// Take it out of the outbox into the composer ([`take_for_edit`]).
    pub can_edit: bool,
    /// Delete it unsent ([`dismiss`]).
    pub can_delete: bool,
    /// The send-now button's words: "Send now", "Retry now" or "Retry".
    pub send_now_label: &'static str,
}

impl RowActions {
    #[must_use]
    pub fn of(kind: RowKind, has_bytes: bool) -> Self {
        let movable = has_bytes && kind != RowKind::Sending;
        Self {
            can_send_now: has_bytes
                && matches!(
                    kind,
                    RowKind::Scheduled | RowKind::Retrying | RowKind::Failed
                ),
            can_reschedule: movable,
            can_edit: movable,
            can_delete: kind != RowKind::Sending,
            send_now_label: match kind {
                RowKind::Retrying => "Retry now",
                RowKind::Failed => "Retry",
                _ => "Send now",
            },
        }
    }
}

/// One outbox row for the outbox view: everything the UI shows, never the
/// MIME bytes (like [`crate::feed`] keeps bodies out of list rows). `kind`
/// and the action flags ([`RowActions`]) say how to show it and what to
/// offer; `state` is the row's one-line state, phrased once here so both
/// frontends show the same words (like [`undo`]'s toast labels). Rows still
/// `scheduled` carry `send_at` and retrying ones `retry_at` (UTC RFC3339,
/// `""` otherwise) for local display; `notify_self` when delivery mails a
/// receipt to the own address. `retryable` is what a sync could deliver
/// now, as in [`status`]: never a row that waits for its time or its pause.
pub fn list_json(db: &Db, account_id: i64) -> Result<String> {
    let mut stmt = db.conn().prepare(
        "select id, status, last_error, retries, raw_mime,
                envelope_from, envelope_to, created_at, updated_at, send_at, notify_self,
                next_attempt_at
          from send_queue
          where account_id = ?1 and status in ('queued', 'sending', 'failed', 'scheduled')
          order by coalesce(send_at, created_at)",
    )?;
    let now = crate::store::now();
    let mut arr = Vec::new();
    let rows = stmt.query_map([account_id], |r| {
        Ok((
            (
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<Vec<u8>>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ),
            (
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
                r.get::<_, Option<String>>(9)?,
                r.get::<_, bool>(10)?,
                r.get::<_, Option<String>>(11)?,
            ),
        ))
    })?;
    for row in rows {
        let (
            (id, status_raw, last_error, retries, raw, from),
            (to_raw, created, updated, send_at, notify_self, next_attempt),
        ) = row?;
        let status = QueueStatus::parse_status(&status_raw);
        let has_bytes = raw.as_deref().is_some_and(|b| !b.is_empty());
        let retries = retries.max(0) as u64;
        let kind = RowKind::of(status, has_bytes, retries);
        let actions = RowActions::of(kind, has_bytes);
        let paused = next_attempt.as_deref().is_some_and(|at| at > now.as_str());
        let retryable = matches!(kind, RowKind::Queued | RowKind::Retrying) && has_bytes && !paused;
        let to: Vec<String> = serde_json::from_str(&to_raw).unwrap_or_default();
        arr.push(json!({
            "id": id,
            "status": status.as_str(),
            "kind": kind.as_str(),
            "state": row_state(kind, has_bytes, retries),
            "last_error": last_error.unwrap_or_default(),
            "retries": retries,
            "retryable": retryable,
            "dismissable": actions.can_delete,
            "has_bytes": has_bytes,
            "actions": actions,
            "envelope_from": from.unwrap_or_default(),
            "envelope_to": to,
            "subject": raw
                .as_deref()
                .and_then(subject_from_mime)
                .unwrap_or_else(|| "(no subject)".to_string()),
            "created_at": created,
            "updated_at": updated,
            "send_at": if kind == RowKind::Scheduled { send_at.unwrap_or_default() } else { String::new() },
            "retry_at": if kind == RowKind::Retrying { next_attempt.unwrap_or_default() } else { String::new() },
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

/// A row's one-line state. `retries` is how many attempts failed so far
/// (set to the cap when a scheduled send gave up early).
#[must_use]
pub fn row_state(kind: RowKind, has_bytes: bool, retries: u64) -> String {
    match kind {
        RowKind::Sending => "Sending…".to_string(),
        RowKind::Scheduled => "Scheduled — sends automatically".to_string(),
        RowKind::Queued if has_bytes => "Queued — goes out with the next sync".to_string(),
        RowKind::Queued => "Incomplete — message data was discarded; delete this entry".to_string(),
        RowKind::Retrying => format!(
            "Failed — tries again by itself (attempt {} of {})",
            retries + 1,
            queue::MAX_SEND_RETRIES
        ),
        RowKind::Failed if has_bytes => {
            "Not sent — stopped retrying; retry, edit or delete it".to_string()
        }
        RowKind::Failed => "Failed — send it again by hand, then delete this entry".to_string(),
    }
}

/// A queued mail taken back into the composer ([`take_for_edit`]): the
/// fields as the composer edits them, the body as editor HTML (inline images
/// as `data:` URIs) and the files staged on disk to re-attach.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OutboxEdit {
    pub from: String,
    pub from_name: String,
    pub to: String,
    pub cc: String,
    /// Recovered from the envelope: a built message carries no `Bcc:`.
    pub bcc: String,
    pub reply_to: String,
    pub subject: String,
    pub editor_html: String,
    pub files: Vec<crate::compose::StagedFile>,
    pub request_mdn: bool,
    pub request_dsn: bool,
}

/// Take an unsent mail of `account_id` out of the outbox to edit it: rebuild
/// the composer's fields from the stored message, stage its files under
/// `base`, then delete the row — in that order, so a failure on the way
/// leaves the mail where it was. Refuses a row being sent right now (the
/// delete is conditional, so a flush that claims it first wins). The
/// composer then holds the only copy: it opens as unsaved work, so closing
/// it asks before discarding.
pub fn take_for_edit(
    db: &Db,
    account_id: i64,
    id: i64,
    base: &std::path::Path,
) -> Result<OutboxEdit> {
    let row = queue::get(db, id)?;
    if row.account_id != account_id {
        return Err(StoreError::NotFound("outbox entry".into()));
    }
    let raw = row
        .raw_mime
        .as_deref()
        .filter(|b| !b.is_empty())
        .ok_or_else(|| {
            StoreError::InvalidInput(
                "this entry has no message data left; send it again by hand".into(),
            )
        })?;
    let (m, parts) = crate::sync::imap::parse_to_new(account_id, 0, 0, &[], raw, true)?;
    let images: Vec<crate::html::InlineImage> = parts
        .iter()
        .filter(|p| p.is_inline)
        .filter_map(|p| {
            Some(crate::html::InlineImage {
                content_id: p.content_id.clone()?,
                mime_type: p.mime_type.clone().unwrap_or_default(),
                data: p.data.clone()?,
            })
        })
        .collect();
    let editor_html = match m.body_html.as_deref().filter(|h| !h.trim().is_empty()) {
        Some(html) => {
            crate::html::inline_cid_images(&crate::html::sanitize_for_send(html), &images).0
        }
        None => m
            .body_text
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .map(crate::html::text_to_html)
            .unwrap_or_default(),
    };
    let shown: Vec<String> = m
        .to_addrs
        .iter()
        .chain(&m.cc_addrs)
        .map(|a| a.to_ascii_lowercase())
        .collect();
    let bcc: Vec<&str> = row
        .envelope_to
        .iter()
        .filter(|a| !shown.contains(&a.to_ascii_lowercase()))
        .map(String::as_str)
        .collect();
    let mut files = Vec::new();
    for (n, part) in parts.iter().enumerate().filter(|(_, p)| !p.is_inline) {
        let Some(data) = part.data.as_deref() else {
            continue;
        };
        let name = crate::paths::safe_attachment_name_for_mime(
            part.filename.as_deref(),
            part.mime_type.as_deref(),
            n as i64,
        );
        files.push(crate::compose::StagedFile {
            path: stage_bytes(base, &name, data)?
                .to_string_lossy()
                .into_owned(),
            name,
        });
    }
    crate::paths::prune_stale_draft_dirs(base);
    let request_mdn = m.raw_headers.as_deref().is_some_and(|h| {
        h.lines().any(|l| {
            l.to_ascii_lowercase()
                .starts_with("disposition-notification-to:")
        })
    });
    let n = db.conn().execute(
        "delete from send_queue
          where id = ?1 and account_id = ?2 and status in ('queued', 'failed', 'scheduled')",
        rusqlite::params![id, account_id],
    )?;
    if n == 0 {
        return Err(StoreError::InvalidInput(
            "this mail is being sent right now".into(),
        ));
    }
    Ok(OutboxEdit {
        from: m.from_addr.unwrap_or_default(),
        from_name: m.from_name.unwrap_or_default(),
        to: m.to_addrs.join(", "),
        cc: m.cc_addrs.join(", "),
        bcc: bcc.join(", "),
        reply_to: m.reply_to.unwrap_or_default(),
        subject: m.subject.unwrap_or_default(),
        editor_html,
        files,
        request_mdn,
        request_dsn: row.request_dsn,
    })
}

/// `data` as `name` in a fresh single-use staging dir under `base` (like a
/// draft's files, see `compose::stage_draft_files`).
fn stage_bytes(base: &std::path::Path, name: &str, data: &[u8]) -> Result<std::path::PathBuf> {
    use std::io::Write;
    let io = |e: std::io::Error| StoreError::InvalidInput(format!("cannot stage a file: {e}"));
    let dir = crate::paths::new_stage_dir(base).map_err(io)?;
    let dest = dir.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(io)?;
    #[cfg(unix)]
    std::fs::set_permissions(&dest, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(io)?;
    file.write_all(data).map_err(io)?;
    Ok(dest)
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
