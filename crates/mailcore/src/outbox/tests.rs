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
    assert_eq!(
        row["state"],
        "Failed — tries again by itself (attempt 2 of 5)"
    );
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
        "Incomplete — message data was discarded; delete this entry"
    );
}

#[test]
fn row_state_names_waiting_retrying_and_dead_rows() {
    use RowKind::*;
    assert_eq!(row_state(Sending, true, 0), "Sending…");
    assert_eq!(
        row_state(Scheduled, true, 0),
        "Scheduled — sends automatically"
    );
    assert_eq!(
        row_state(Queued, true, 0),
        "Queued — goes out with the next sync"
    );
    assert_eq!(
        row_state(Queued, false, 0),
        "Incomplete — message data was discarded; delete this entry"
    );
    assert_eq!(
        row_state(Retrying, true, 1),
        "Failed — tries again by itself (attempt 2 of 5)"
    );
    assert_eq!(
        row_state(Failed, true, 5),
        "Not sent — stopped retrying; retry, edit or delete it"
    );
    assert_eq!(
        row_state(Failed, false, 0),
        "Failed — send it again by hand, then delete this entry"
    );
}

#[test]
fn row_actions_follow_kind_and_bytes() {
    let sending = RowActions::of(RowKind::Sending, true);
    assert!(!sending.can_send_now && !sending.can_edit && !sending.can_delete);
    let held = RowActions::of(RowKind::Scheduled, true);
    assert!(held.can_send_now && held.can_reschedule && held.can_edit && held.can_delete);
    assert_eq!(held.send_now_label, "Send now");
    let queued = RowActions::of(RowKind::Queued, true);
    assert!(!queued.can_send_now && queued.can_edit);
    assert_eq!(
        RowActions::of(RowKind::Retrying, true).send_now_label,
        "Retry now"
    );
    let dead = RowActions::of(RowKind::Failed, true);
    assert!(dead.can_send_now && dead.can_edit);
    assert_eq!(dead.send_now_label, "Retry");
    let empty = RowActions::of(RowKind::Failed, false);
    assert!(!empty.can_send_now && !empty.can_edit && !empty.can_reschedule && empty.can_delete);
}

#[test]
fn a_scheduled_send_that_gave_up_lists_as_failed_with_retry() {
    let db = Db::open_in_memory().unwrap();
    let acc = account(&db);
    let id = scheduled_row(
        &db,
        acc,
        b"From: me@example.com\r\nTo: you@example.com\r\nSubject: late\r\n\r\nbody",
    );
    queue::fail_scheduled_attempt(&db, id, "timeout", false).unwrap();
    let list: serde_json::Value = serde_json::from_str(&list_json(&db, acc).unwrap()).unwrap();
    assert_eq!(list[0]["kind"], "retrying");
    assert_ne!(list[0]["retry_at"], "");
    // In its pause: nothing a sync could deliver now.
    assert_eq!(list[0]["retryable"], false);
    queue::fail_scheduled_attempt(&db, id, "550 refused", true).unwrap();
    let list: serde_json::Value = serde_json::from_str(&list_json(&db, acc).unwrap()).unwrap();
    assert_eq!(list[0]["kind"], "failed");
    assert_eq!(list[0]["retry_at"], "");
    assert_eq!(list[0]["last_error"], "550 refused");
    assert_eq!(list[0]["actions"]["send_now_label"], "Retry");
    assert_eq!(list[0]["actions"]["can_edit"], true);
}

#[test]
fn edit_takes_the_mail_back_with_bcc_files_and_inline_images() {
    let db = Db::open_in_memory().unwrap();
    let acc = account(&db);
    let raw = concat!(
        "From: Me <me@example.com>\r\n",
        "To: you@example.com\r\n",
        "Cc: cc@example.org\r\n",
        "Reply-To: replies@example.com\r\n",
        "Disposition-Notification-To: me@example.com\r\n",
        "Subject: =?utf-8?q?Pr=C3=BCfung?=\r\n",
        "MIME-Version: 1.0\r\n",
        "Content-Type: multipart/mixed; boundary=\"outer\"\r\n",
        "\r\n",
        "--outer\r\n",
        "Content-Type: multipart/related; boundary=\"rel\"\r\n",
        "\r\n",
        "--rel\r\n",
        "Content-Type: text/html; charset=utf-8\r\n",
        "\r\n",
        "<p>Hello <img src=\"cid:logo@example.com\"></p>\r\n",
        "--rel\r\n",
        "Content-Type: image/png\r\n",
        "Content-ID: <logo@example.com>\r\n",
        "Content-Disposition: inline\r\n",
        "Content-Transfer-Encoding: base64\r\n",
        "\r\n",
        "iVBORw0KGgo=\r\n",
        "--rel--\r\n",
        "--outer\r\n",
        "Content-Type: text/plain; name=\"notes.txt\"\r\n",
        "Content-Disposition: attachment; filename=\"notes.txt\"\r\n",
        "\r\n",
        "file body\r\n",
        "--outer--\r\n",
    );
    let at = (chrono::Utc::now() + chrono::Duration::hours(2))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let to = [
        "you@example.com".to_string(),
        "cc@example.org".to_string(),
        "hidden@example.org".to_string(),
    ];
    let id = queue::enqueue_scheduled_mime(
        &db,
        acc,
        &queue::ScheduledMime {
            message_id: None,
            raw_mime: raw.as_bytes(),
            envelope_from: "me@example.com",
            envelope_to: &to,
            request_dsn: true,
            notify_self: false,
            send_at: &at,
        },
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    // Not someone else's.
    assert!(take_for_edit(&db, acc + 99, id, dir.path()).is_err());
    let e = take_for_edit(&db, acc, id, dir.path()).unwrap();
    assert_eq!(e.from, "me@example.com");
    assert_eq!(e.from_name, "Me");
    assert_eq!(e.to, "you@example.com");
    assert_eq!(e.cc, "cc@example.org");
    assert_eq!(e.bcc, "hidden@example.org");
    assert_eq!(e.reply_to, "replies@example.com");
    assert_eq!(e.subject, "Pr\u{fc}fung");
    assert!(e.request_mdn && e.request_dsn);
    assert!(
        e.editor_html.contains("data:image/png;base64,"),
        "{}",
        e.editor_html
    );
    assert_eq!(e.files.len(), 1);
    assert_eq!(e.files[0].name, "notes.txt");
    assert_eq!(
        std::fs::read_to_string(&e.files[0].path).unwrap().trim(),
        "file body"
    );
    // Out of the outbox now: the composer holds the only copy.
    assert!(queue::get(&db, id).is_err());
}

#[test]
fn edit_refuses_a_row_being_sent() {
    let db = Db::open_in_memory().unwrap();
    let acc = account(&db);
    let id = enqueue(&db, acc, "in flight");
    let dir = tempfile::tempdir().unwrap();
    let err = take_for_edit(&db, acc, id, dir.path()).unwrap_err();
    assert!(err.to_string().contains("being sent"), "{err}");
    assert!(queue::get(&db, id).is_ok());
}

fn scheduled_row(db: &Db, acc: i64, raw: &[u8]) -> i64 {
    let at = (chrono::Utc::now() + chrono::Duration::hours(2))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let to = ["you@example.com".to_string()];
    queue::enqueue_scheduled_mime(
        db,
        acc,
        &queue::ScheduledMime {
            message_id: None,
            raw_mime: raw,
            envelope_from: "me@example.com",
            envelope_to: &to,
            request_dsn: false,
            notify_self: false,
            send_at: &at,
        },
    )
    .unwrap()
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
