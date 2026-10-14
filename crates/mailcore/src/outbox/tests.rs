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
    let raw =
        format!("From: me@example.com\r\nTo: you@example.com\r\nSubject: {subject}\r\n\r\nbody");
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
            scheduled: 0,
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
    // A scheduled send counts apart from the retryable queue.
    let at = (chrono::Utc::now() + chrono::Duration::hours(2))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let to = ["you@example.com".to_string()];
    queue::enqueue_scheduled_mime(
        &db,
        acc,
        &queue::ScheduledMime {
            message_id: None,
            raw_mime: b"From: me@example.com\r\nTo: you@example.com\r\nSubject: later\r\n\r\nbody",
            envelope_from: "me@example.com",
            envelope_to: &to,
            request_dsn: false,
            notify_self: false,
            send_at: &at,
        },
    )
    .unwrap();
    let s = status(&db, acc).unwrap();
    assert_eq!((s.scheduled, s.retryable, s.pending), (1, 0, 2));
    assert_eq!(s.label(), "2 unsent (1 failed, 1 scheduled)");
    // Sent rows leave the counts.
    queue::mark_sent(&db, a).unwrap();
    let s = status(&db, acc).unwrap();
    assert_eq!((s.failed, s.scheduled, s.pending), (0, 1, 1));
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
fn scheduled_rows_carry_send_at_and_reschedule() {
    let db = Db::open_in_memory().unwrap();
    let acc = account(&db);
    let at = (chrono::Utc::now() + chrono::Duration::hours(2))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let to = ["you@example.com".to_string()];
    let scheduled = |subject: &[u8], send_at: &str| {
        queue::enqueue_scheduled_mime(
            &db,
            acc,
            &queue::ScheduledMime {
                message_id: None,
                raw_mime: subject,
                envelope_from: "me@example.com",
                envelope_to: &to,
                request_dsn: false,
                notify_self: false,
                send_at,
            },
        )
        .unwrap()
    };
    let id = scheduled(
        b"From: me@example.com\r\nTo: you@example.com\r\nSubject: later\r\n\r\nbody",
        &at,
    );
    let list: serde_json::Value = serde_json::from_str(&list_json(&db, acc).unwrap()).unwrap();
    assert_eq!(list[0]["status"], "scheduled");
    assert_eq!(list[0]["state"], "Scheduled — sends automatically");
    assert_eq!(list[0]["send_at"], at);
    assert_eq!(list[0]["dismissable"], true);
    // Waiting for its time: nothing a sync could deliver now.
    assert_eq!(list[0]["retryable"], false);
    let due = due_in_ms(&db);
    assert!(due > 3_600_000 && due <= 7_200_000, "{due}");
    // Someone else's account cannot touch it.
    assert!(reschedule(&db, acc + 99, id, &at).is_err());
    assert!(send_now(&db, acc + 99, id).is_err());
    // A new time moves it; Send now queues it for the next sync.
    let later = (chrono::Utc::now() + chrono::Duration::hours(5))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    assert_eq!(reschedule(&db, acc, id, &later).unwrap(), later);
    assert!(reschedule(&db, acc, id, "yesterday").is_err());
    send_now(&db, acc, id).unwrap();
    let list: serde_json::Value = serde_json::from_str(&list_json(&db, acc).unwrap()).unwrap();
    assert_eq!(list[0]["status"], "queued");
    // No "Sends at" line once it left `scheduled`; due right away.
    assert_eq!(list[0]["send_at"], "");
    assert_eq!(due_in_ms(&db), 0);
    // And a scheduled row dismisses like any other held row.
    let id2 = scheduled(
        b"From: me@example.com\r\nTo: you@example.com\r\nSubject: never\r\n\r\nbody",
        &later,
    );
    dismiss(&db, acc, id2).unwrap();
    assert!(queue::get(&db, id2).is_err());
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
        state_line(Scheduled, false, true),
        "Scheduled — sends automatically"
    );
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
        subject_from_mime(b"subject: folded\r\n continues here\r\nTo: a@b.c\r\n\r\nx").as_deref(),
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
        scheduled: 0,
        retryable: 1,
        pending: 1,
    };
    assert_eq!(one.label(), "1 unsent");
    let sched = OutboxStatus {
        scheduled: 2,
        pending: 2,
        ..one
    };
    assert_eq!(sched.label(), "2 unsent (2 scheduled)");
}

#[test]
fn send_at_error_names_the_lead_time_rule() {
    assert!(send_at_error("not a time")
        .unwrap()
        .contains("valid date and time"));
    let soon = (chrono::Utc::now() + chrono::Duration::seconds(10)).to_rfc3339();
    assert!(send_at_error(&soon).unwrap().contains("at least a minute"));
    let later = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    assert_eq!(send_at_error(&later), None);
}

#[test]
fn nothing_scheduled_plans_nothing() {
    let db = Db::open_in_memory().unwrap();
    account(&db);
    assert_eq!(due_in_ms(&db), -1);
}

#[test]
fn encoded_subjects_are_decoded() {
    assert_eq!(
        subject_from_mime(b"Subject: =?utf-8?q?Pr=C3=BCfung?=\r\n\r\nx").as_deref(),
        Some("Pr\u{fc}fung")
    );
    assert_eq!(subject_from_mime(b"Subject: \r\n\r\nx"), None);
}
