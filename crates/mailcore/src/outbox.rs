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
/// number (`queued + sending + failed`); `failed` drives the error styling;
/// `retryable` (bytes present, retries left) says whether a sync could still
/// deliver something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct OutboxStatus {
    pub queued: u64,
    pub sending: u64,
    pub failed: u64,
    pub retryable: u64,
    pub pending: u64,
}

impl OutboxStatus {
    /// Whether the pill shows at all.
    #[must_use]
    pub fn any(self) -> bool {
        self.pending > 0
    }

    /// The pill's words, phrased once for both frontends: `2 unsent` or
    /// `2 unsent (1 failed)`.
    #[must_use]
    pub fn label(self) -> String {
        if self.failed > 0 {
            format!("{} unsent ({} failed)", self.pending, self.failed)
        } else {
            format!("{} unsent", self.pending)
        }
    }
}

/// Count the account's outbox rows by status. Rows marked `queued` that have
/// no MIME bytes left are counted under `failed`, since without bytes they can
/// never be submitted.
pub fn status(db: &Db, account_id: i64) -> Result<OutboxStatus> {
    let mut queued = 0u64;
    let mut sending = 0u64;
    let mut failed = 0u64;
    {
        let mut stmt = db.conn().prepare(
            "select case
                      when status = 'queued' and (raw_mime is null or length(raw_mime) = 0) then 'failed'
                      else status
                    end,
                    count(*)
               from send_queue
              where account_id = ?1 and status in ('queued', 'sending', 'failed')
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
                QueueStatus::Sent => {}
            }
        }
    }
    // Same conditions as `queue::list_submittable`, without loading the
    // MIME bytes of every queued message.
    let retryable = db.conn().query_row(
        "select count(*) from send_queue
          where account_id = ?1
            and status in ('queued', 'failed')
            and raw_mime is not null and length(raw_mime) > 0
            and retries < ?2",
        rusqlite::params![account_id, queue::MAX_SEND_RETRIES as i64],
        |r| r.get::<_, i64>(0),
    )? as u64;
    Ok(OutboxStatus {
        queued,
        sending,
        failed,
        retryable,
        pending: queued + sending + failed,
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
/// same words (like [`crate::undo`]'s toast labels).
pub fn list_json(db: &Db, account_id: i64) -> Result<String> {
    let mut stmt = db.conn().prepare(
        "select id, status, last_error, retries, raw_mime,
                envelope_from, envelope_to, created_at, updated_at
          from send_queue
          where account_id = ?1 and status in ('queued', 'sending', 'failed')
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
        ))
    })?;
    for row in rows {
        let (id, status_raw, last_error, retries, raw, from, to_raw, created, updated) = row?;
        let status = QueueStatus::parse_status(&status_raw);
        let has_bytes = raw.as_deref().is_some_and(|b| !b.is_empty());
        // `dismiss`'s rule: never a row being sent.
        let dismissable = matches!(status, QueueStatus::Queued | QueueStatus::Failed);
        let retryable = dismissable && has_bytes && (retries as u64) < queue::MAX_SEND_RETRIES;
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
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

/// Forget one outbox row of this account (a failed send the user owns the
/// retry for, or a stale entry). Fails with `NotFound` when the id is gone,
/// belongs to another account, or is `sending` right now — deleting that
/// would not stop the mail, only hide that it went out. `sent` rows are
/// [`queue::prune_sent`]'s job.
pub fn dismiss(db: &Db, account_id: i64, id: i64) -> Result<()> {
    let n = db.conn().execute(
        "delete from send_queue
          where id = ?1 and account_id = ?2 and status in ('queued', 'failed')",
        rusqlite::params![id, account_id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound(
            "outbox entry (already sent, being sent, or gone)".into(),
        ));
    }
    Ok(())
}

/// A row's one-line state from its status, whether a sync could still
/// deliver it (bytes present, retries left), and whether raw MIME bytes exist.
#[must_use]
pub fn state_line(status: QueueStatus, retryable: bool, has_bytes: bool) -> &'static str {
    match status {
        QueueStatus::Sending => "Sending…",
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

/// The `Subject:` header of a built MIME message, unfolded. No RFC 2047
/// decoding — an encoded word shows as-is, which is enough for recognising
/// one's own queued mail.
fn subject_from_mime(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let mut subject: Option<String> = None;
    for line in text.split_inclusive('\n') {
        let line = line.strip_suffix('\n').unwrap_or(line);
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            break; // end of headers
        }
        if line.starts_with([' ', '\t']) {
            if let Some(s) = subject.as_mut() {
                s.push(' ');
                s.push_str(line.trim());
            }
            continue;
        }
        if is_subject_header(line) {
            let value = line.split_once(':').map(|(_, v)| v.trim()).unwrap_or("");
            subject = Some(value.to_string());
        }
    }
    subject.filter(|s| !s.is_empty())
}

fn is_subject_header(line: &str) -> bool {
    line.len() > 8 && line.as_bytes()[7] == b':' && line[..7].eq_ignore_ascii_case("subject")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAccount;
    use crate::store::accounts;

    fn account(db: &Db) -> i64 {
        accounts::create(
            db,
            &NewAccount {
                name: "a".to_string(),
                email_address: "me@example.com".to_string(),
                from_name: String::new(),
                imap_host: "h".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "h".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap()
    }

    fn enqueue(db: &Db, acc: i64, subject: &str) -> i64 {
        let raw = format!(
            "From: me@example.com\r\nTo: you@example.com\r\nSubject: {subject}\r\n\r\nbody"
        );
        queue::enqueue_mime(
            db,
            acc,
            None,
            raw.as_bytes(),
            "me@example.com",
            &["you@example.com".to_string()],
            false,
        )
        .unwrap()
    }

    #[test]
    fn empty_outbox_has_no_status() {
        let db = Db::open_in_memory().unwrap();
        let acc = account(&db);
        let s = status(&db, acc).unwrap();
        assert_eq!(
            s,
            OutboxStatus {
                queued: 0,
                sending: 0,
                failed: 0,
                retryable: 0,
                pending: 0,
            }
        );
        assert!(!s.any());
        assert_eq!(list_json(&db, acc).unwrap(), "[]");
    }

    #[test]
    fn counts_follow_row_states() {
        let db = Db::open_in_memory().unwrap();
        let acc = account(&db);
        // Born claimed (`sending`).
        let a = enqueue(&db, acc, "first");
        let s = status(&db, acc).unwrap();
        assert_eq!((s.sending, s.retryable, s.pending), (1, 0, 1));
        assert!(s.any());
        // A failed flush keeps its bytes: retryable.
        queue::mark_failed(&db, a, "timeout").unwrap();
        let s = status(&db, acc).unwrap();
        assert_eq!((s.failed, s.retryable), (1, 1));
        // A user-reported failure drops its bytes: visible but not retryable.
        queue::discard_mime(&db, a).unwrap();
        let s = status(&db, acc).unwrap();
        assert_eq!((s.failed, s.retryable), (1, 0));
        // Sent rows leave the counts.
        queue::mark_sent(&db, a).unwrap();
        assert!(!status(&db, acc).unwrap().any());
    }

    #[test]
    fn list_rows_carry_subject_recipients_and_error() {
        let db = Db::open_in_memory().unwrap();
        let acc = account(&db);
        let id = enqueue(&db, acc, "hello there");
        queue::mark_failed(&db, id, "connection refused").unwrap();
        let list: serde_json::Value = serde_json::from_str(&list_json(&db, acc).unwrap()).unwrap();
        let row = &list[0];
        assert_eq!(row["id"], id);
        assert_eq!(row["status"], "failed");
        assert_eq!(row["state"], "Failed — will retry on the next sync");
        assert_eq!(row["last_error"], "connection refused");
        assert_eq!(row["retryable"], true);
        assert_eq!(row["has_bytes"], true);
        assert_eq!(row["subject"], "hello there");
        assert_eq!(row["envelope_to"][0], "you@example.com");
    }

    #[test]
    fn dismiss_is_scoped_to_the_account() {
        let db = Db::open_in_memory().unwrap();
        let acc = account(&db);
        let other = accounts::create(
            &db,
            &NewAccount {
                name: "b".to_string(),
                email_address: "b@example.com".to_string(),
                from_name: String::new(),
                imap_host: "h".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "h".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k2".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let id = enqueue(&db, other, "theirs");
        queue::mark_failed(&db, id, "connection refused").unwrap();
        assert!(dismiss(&db, acc, id).is_err());
        dismiss(&db, other, id).unwrap();
        assert!(dismiss(&db, other, id).is_err());
        assert!(dismiss(&db, acc, 9999).is_err());
    }

    #[test]
    fn a_row_being_sent_cannot_be_dismissed() {
        let db = Db::open_in_memory().unwrap();
        let acc = account(&db);
        // Born claimed: a sync is submitting it right now.
        let id = enqueue(&db, acc, "in flight");
        assert!(dismiss(&db, acc, id).is_err());
        assert!(queue::get(&db, id).is_ok());
        let rows: serde_json::Value = serde_json::from_str(&list_json(&db, acc).unwrap()).unwrap();
        assert_eq!(rows[0]["dismissable"], false);
    }

    #[test]
    fn queued_without_bytes_counts_as_failed() {
        let db = Db::open_in_memory().unwrap();
        let acc = account(&db);
        let id = enqueue(&db, acc, "empty queued");
        queue::discard_mime(&db, id).unwrap();
        db.conn()
            .execute(
                "update send_queue set status = 'queued' where id = ?1",
                [id],
            )
            .unwrap();
        let s = status(&db, acc).unwrap();
        assert_eq!(s.queued, 0);
        assert_eq!(s.failed, 1);
        assert_eq!(s.retryable, 0);
        assert_eq!(s.pending, 1);
        let list: serde_json::Value = serde_json::from_str(&list_json(&db, acc).unwrap()).unwrap();
        assert_eq!(
            list[0]["state"],
            "Incomplete — message data was discarded; dismiss this entry"
        );
    }

    #[test]
    fn state_line_names_retryable_and_dead_rows() {
        use QueueStatus::*;
        assert_eq!(state_line(Sending, false, true), "Sending…");
        assert_eq!(
            state_line(Queued, true, true),
            "Queued — goes out with the next sync"
        );
        assert_eq!(
            state_line(Queued, false, false),
            "Incomplete — message data was discarded; dismiss this entry"
        );
        assert_eq!(
            state_line(Failed, true, true),
            "Failed — will retry on the next sync"
        );
        assert_eq!(
            state_line(Failed, false, true),
            "Failed — retry limit reached, dismiss or resend by hand"
        );
        assert_eq!(
            state_line(Failed, false, false),
            "Failed — send it again by hand, then dismiss this entry"
        );
    }

    #[test]
    fn subject_parsing_handles_folding_case_and_absence() {
        assert_eq!(
            subject_from_mime(b"Subject: Hi\r\n\r\nx").as_deref(),
            Some("Hi")
        );
        assert_eq!(
            subject_from_mime(b"subject: folded\r\n continues here\r\nTo: a@b.c\r\n\r\nx")
                .as_deref(),
            Some("folded continues here")
        );
        assert_eq!(
            subject_from_mime(b"From: a@b.c\r\n\r\nno subject here"),
            None
        );
        // Headers end at the blank line: no false subject from the body.
        assert_eq!(
            subject_from_mime(b"From: a@b.c\r\n\r\nSubject: not a header"),
            None
        );
    }

    #[test]
    fn status_json_carries_the_pill_label() {
        let db = Db::open_in_memory().unwrap();
        let acc = account(&db);
        let a = enqueue(&db, acc, "one");
        enqueue(&db, acc, "two");
        queue::mark_failed(&db, a, "refused").unwrap();
        let v: serde_json::Value = serde_json::from_str(&status_json(&db, acc).unwrap()).unwrap();
        assert_eq!(v["label"], "2 unsent (1 failed)");
        assert_eq!(v["has_failures"], true);
        let one = OutboxStatus {
            queued: 1,
            sending: 0,
            failed: 0,
            retryable: 1,
            pending: 1,
        };
        assert_eq!(one.label(), "1 unsent");
    }
}
