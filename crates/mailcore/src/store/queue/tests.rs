use super::*;
use crate::models::NewAccount;
use crate::store::accounts;

const RAW: &[u8] = b"From: a@example.com\r\nTo: b@example.org\r\nSubject: hi\r\n\r\nbody";

fn account(db: &Db, name: &str, vault_key: &str) -> i64 {
    accounts::create(
        db,
        &NewAccount {
            name: name.to_string(),
            email_address: format!("{name}@example.com"),
            from_name: String::new(),
            imap_host: "h".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "u".to_string(),
            smtp_host: "h".to_string(),
            smtp_port: 465,
            smtp_security: "tls".to_string(),
            smtp_username: "u".to_string(),
            auth_vault_key: vault_key.to_string(),
            check_interval_secs: 60,
        },
    )
    .unwrap()
}

fn setup() -> (Db, i64) {
    let db = Db::open_in_memory().unwrap();
    let acc = account(&db, "a", "k");
    (db, acc)
}

fn enqueue(db: &Db, acc: i64) -> i64 {
    enqueue_mime(
        db,
        acc,
        None,
        RAW,
        "a@example.com",
        &["b@example.org".to_string()],
        false,
    )
    .unwrap()
}

/// Backdate a row so it looks orphaned by a crash long ago.
fn age(db: &Db, id: i64, minutes: i64) {
    let ts = (chrono::Utc::now() - chrono::Duration::minutes(minutes))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    db.conn()
        .execute(
            "update send_queue set updated_at = ?1 where id = ?2",
            params![ts, id],
        )
        .unwrap();
}

#[test]
fn enqueue_and_fail_then_sent() {
    let (db, acc) = setup();
    let id = enqueue(&db, acc);
    assert_eq!(list_pending(&db).unwrap().len(), 1);
    mark_failed(&db, id, "connection refused").unwrap();
    let pending = list_pending(&db).unwrap();
    assert_eq!(pending[0].retries, 1);
    assert_eq!(pending[0].status, QueueStatus::Failed);
    mark_sent(&db, id).unwrap();
    assert!(list_pending(&db).unwrap().is_empty());
}

#[test]
fn mime_bytes_roundtrip_and_born_claimed() {
    let (db, acc) = setup();
    let id = enqueue(&db, acc);
    let row = get(&db, id).unwrap();
    assert_eq!(row.status, QueueStatus::Sending);
    assert_eq!(row.raw_mime.as_deref(), Some(RAW));
    assert_eq!(row.envelope_from.as_deref(), Some("a@example.com"));
    assert_eq!(row.envelope_to, vec!["b@example.org".to_string()]);
    assert!(!row.request_dsn);
    let dsn = enqueue_mime(
        &db,
        acc,
        None,
        RAW,
        "a@example.com",
        &["b@example.org".to_string()],
        true,
    )
    .unwrap();
    assert!(get(&db, dsn).unwrap().request_dsn);
    // Owned by the enqueuer: a concurrent flush must not see it.
    assert!(list_submittable(&db, acc).unwrap().is_empty());
    assert!(!claim(&db, id).unwrap());
}

#[test]
fn only_one_submitter_wins_the_claim() {
    let (db, acc) = setup();
    let id = enqueue(&db, acc);
    mark_failed(&db, id, "timeout").unwrap();
    assert_eq!(list_submittable(&db, acc).unwrap().len(), 1);
    assert!(claim(&db, id).unwrap());
    assert!(!claim(&db, id).unwrap());
    assert!(list_submittable(&db, acc).unwrap().is_empty());
}

#[test]
fn a_live_send_is_not_requeued_but_an_orphan_is() {
    let (db, acc) = setup();
    let live = enqueue(&db, acc);
    let orphan = enqueue(&db, acc);
    age(&db, orphan, STALE_SENDING_MINUTES + 1);
    assert_eq!(requeue_interrupted(&db, acc).unwrap(), 1);
    assert_eq!(get(&db, live).unwrap().status, QueueStatus::Sending);
    assert_eq!(get(&db, orphan).unwrap().status, QueueStatus::Queued);
}

#[test]
fn an_orphan_without_bytes_becomes_failed_not_queued() {
    let (db, acc) = setup();
    let orphan = enqueue(&db, acc);
    discard_mime(&db, orphan).unwrap();
    age(&db, orphan, STALE_SENDING_MINUTES + 1);
    assert_eq!(requeue_interrupted(&db, acc).unwrap(), 0);
    let row = get(&db, orphan).unwrap();
    assert_eq!(row.status, QueueStatus::Failed);
}

#[test]
fn marking_sent_releases_the_mime() {
    let (db, acc) = setup();
    let id = enqueue(&db, acc);
    mark_sent(&db, id).unwrap();
    let row = get(&db, id).unwrap();
    assert_eq!(row.status, QueueStatus::Sent);
    assert_eq!(row.raw_mime, None);
}

#[test]
fn discarded_mime_is_never_resubmitted() {
    let (db, acc) = setup();
    let id = enqueue(&db, acc);
    mark_failed(&db, id, "550 rejected").unwrap();
    discard_mime(&db, id).unwrap();
    assert!(list_submittable(&db, acc).unwrap().is_empty());
    // Still visible as a pending outbox problem, just not auto-retried.
    assert_eq!(list_pending(&db).unwrap().len(), 1);
}

#[test]
fn retry_cap_stops_resubmission() {
    let (db, acc) = setup();
    let id = enqueue(&db, acc);
    for _ in 0..MAX_SEND_RETRIES {
        mark_failed(&db, id, "timeout").unwrap();
        if get(&db, id).unwrap().retries < MAX_SEND_RETRIES {
            assert_eq!(list_submittable(&db, acc).unwrap().len(), 1);
            assert!(claim(&db, id).unwrap());
        }
    }
    assert!(list_submittable(&db, acc).unwrap().is_empty());
}

#[test]
fn requeue_interrupted_is_scoped_to_the_account() {
    let (db, acc) = setup();
    let other = account(&db, "b", "k2");
    let mine = enqueue(&db, acc);
    let theirs = enqueue(&db, other);
    age(&db, mine, STALE_SENDING_MINUTES + 1);
    age(&db, theirs, STALE_SENDING_MINUTES + 1);
    assert_eq!(requeue_interrupted(&db, acc).unwrap(), 1);
    assert_eq!(get(&db, mine).unwrap().status, QueueStatus::Queued);
    assert_eq!(get(&db, theirs).unwrap().status, QueueStatus::Sending);
}

#[test]
fn prune_keeps_recent_sent_rows() {
    let (db, acc) = setup();
    let recent = enqueue(&db, acc);
    let old = enqueue(&db, acc);
    mark_sent(&db, recent).unwrap();
    mark_sent(&db, old).unwrap();
    db.conn()
        .execute(
            "update send_queue set updated_at = ?1 where id = ?2",
            params![days_ago(SENT_RETENTION_DAYS + 1), old],
        )
        .unwrap();
    assert_eq!(prune_sent(&db).unwrap(), 1);
    assert!(get(&db, recent).is_ok());
    assert!(get(&db, old).is_err());
}

fn future_ts(minutes: i64) -> String {
    (chrono::Utc::now() + chrono::Duration::minutes(minutes))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn schedule(db: &Db, acc: i64, minutes: i64) -> i64 {
    let at = future_ts(minutes);
    let to = vec!["b@example.org".to_string()];
    enqueue_scheduled_mime(
        db,
        acc,
        &ScheduledMime {
            message_id: None,
            raw_mime: RAW,
            envelope_from: "a@example.com",
            envelope_to: &to,
            request_dsn: false,
            notify_self: false,
            send_at: &at,
        },
    )
    .unwrap()
}

#[test]
fn scheduled_rows_wait_unclaimed_until_due() {
    let (db, acc) = setup();
    let id = schedule(&db, acc, 60);
    let row = get(&db, id).unwrap();
    assert_eq!(row.status, QueueStatus::Scheduled);
    assert!(row.send_at.is_some());
    // Held back: not submittable, not claimable, but listed as pending.
    assert!(list_submittable(&db, acc).unwrap().is_empty());
    assert!(!claim(&db, id).unwrap());
    assert_eq!(list_pending(&db).unwrap().len(), 1);
    // Future validation: past and near times are refused; other offsets
    // normalize to the same stored instant.
    assert!(normalize_future_send_at("not a time").is_err());
    assert!(normalize_future_send_at(&future_ts(-5)).is_err());
    assert!(normalize_future_send_at(&future_ts(0)).is_err());
    let utc = chrono::Utc::now() + chrono::Duration::minutes(90);
    let east = chrono::FixedOffset::east_opt(7200).unwrap();
    let shifted = utc
        .with_timezone(&east)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    assert!(shifted.ends_with("+02:00"));
    assert_eq!(
        normalize_future_send_at(&shifted).unwrap(),
        utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    );
}

#[test]
fn promote_due_moves_only_past_rows_and_keeps_send_at() {
    let (db, acc) = setup();
    let due = schedule(&db, acc, 60);
    let later = schedule(&db, acc, 120);
    // Backdate one past its time: only it promotes.
    let past = (chrono::Utc::now() - chrono::Duration::minutes(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    db.conn()
        .execute(
            "update send_queue set send_at = ?1 where id = ?2",
            params![past, due],
        )
        .unwrap();
    assert_eq!(promote_due(&db, acc).unwrap(), 1);
    let row = get(&db, due).unwrap();
    assert_eq!(row.status, QueueStatus::Queued);
    assert_eq!(row.send_at.as_deref(), Some(past.as_str()));
    assert_eq!(get(&db, later).unwrap().status, QueueStatus::Scheduled);
    // The due row submits now; the future one still waits.
    assert_eq!(
        list_submittable(&db, acc)
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        vec![due]
    );
}

/// Move a row's `send_at` (and `updated_at`) `minutes` into the past.
fn backdate(db: &Db, id: i64, minutes: i64) -> String {
    let past = (chrono::Utc::now() - chrono::Duration::minutes(minutes))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    db.conn()
        .execute(
            "update send_queue set send_at = ?1, updated_at = ?1 where id = ?2",
            params![past, id],
        )
        .unwrap();
    past
}

#[test]
fn reschedule_rejects_the_past_and_send_now_sends_at_once() {
    let (db, acc) = setup();
    let id = schedule(&db, acc, 60);
    assert!(reschedule(&db, acc, id, &future_ts(-10)).is_err());
    let moved = reschedule(&db, acc, id, &future_ts(180)).unwrap();
    let row = get(&db, id).unwrap();
    assert_eq!(row.status, QueueStatus::Scheduled);
    assert_eq!(row.send_at.as_deref(), Some(moved.as_str()));
    send_now(&db, acc, id).unwrap();
    let row = get(&db, id).unwrap();
    assert_eq!(row.status, QueueStatus::Queued);
    // Still timed (now), so delivery refreshes the stale `Date:`.
    let at = row.send_at.expect("send now keeps a send time");
    assert!(at <= now());
    assert_eq!(list_submittable(&db, acc).unwrap().len(), 1);
    // ...and the planners flush it without waiting for a sync.
    assert_eq!(due_scheduled_account_ids(&db).unwrap(), vec![acc]);
    // A live send cannot be rescheduled or rushed.
    let live = enqueue(&db, acc);
    assert!(reschedule(&db, acc, live, &future_ts(180)).is_err());
    assert!(send_now(&db, acc, live).is_err());
}

#[test]
fn reschedule_and_send_now_are_scoped_to_the_account() {
    let (db, acc) = setup();
    let other = account(&db, "b", "k2");
    let id = schedule(&db, acc, 60);
    assert!(matches!(
        reschedule(&db, other, id, &future_ts(120)),
        Err(StoreError::NotFound(_))
    ));
    assert!(matches!(
        send_now(&db, other, id),
        Err(StoreError::NotFound(_))
    ));
    assert_eq!(get(&db, id).unwrap().status, QueueStatus::Scheduled);
}

#[test]
fn a_row_claimed_by_a_flush_is_not_pulled_back() {
    let (db, acc) = setup();
    let id = schedule(&db, acc, 60);
    backdate(&db, id, 1);
    promote_due(&db, acc).unwrap();
    assert!(claim(&db, id).unwrap());
    // A submit owns it now: neither action may hand it to a second one.
    assert!(send_now(&db, acc, id).is_err());
    assert!(reschedule(&db, acc, id, &future_ts(60)).is_err());
    assert_eq!(get(&db, id).unwrap().status, QueueStatus::Sending);
}

#[test]
fn reschedule_resets_an_exhausted_retry_budget() {
    let (db, acc) = setup();
    let id = schedule(&db, acc, 60);
    backdate(&db, id, 1);
    promote_due(&db, acc).unwrap();
    for _ in 0..MAX_SEND_RETRIES {
        mark_failed(&db, id, "timeout").unwrap();
    }
    assert!(list_submittable(&db, acc).unwrap().is_empty());
    reschedule(&db, acc, id, &future_ts(60)).unwrap();
    let row = get(&db, id).unwrap();
    assert_eq!((row.retries, row.last_error), (0, None));
    send_now(&db, acc, id).unwrap();
    assert_eq!(list_submittable(&db, acc).unwrap().len(), 1);
}

#[test]
fn reschedule_refuses_a_row_without_bytes() {
    let (db, acc) = setup();
    let id = schedule(&db, acc, 60);
    discard_mime(&db, id).unwrap();
    let err = reschedule(&db, acc, id, &future_ts(120)).unwrap_err();
    assert!(err.to_string().contains("no message data"), "{err}");
}

#[test]
fn next_due_reports_the_earliest_held_instant() {
    let (db, acc) = setup();
    assert_eq!(next_due(&db).unwrap(), None);
    schedule(&db, acc, 120);
    let first = schedule(&db, acc, 60);
    let at = next_due(&db).unwrap().unwrap();
    assert_eq!(
        at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        get(&db, first).unwrap().send_at.unwrap()
    );
}

#[test]
fn due_scheduled_lists_only_accounts_past_their_time() {
    let (db, acc) = setup();
    assert!(due_scheduled_account_ids(&db).unwrap().is_empty());
    schedule(&db, acc, 120);
    // Still in the future: nobody is due.
    assert!(due_scheduled_account_ids(&db).unwrap().is_empty());
    // Backdate one row past its time: its account is due.
    let id = schedule(&db, acc, 60);
    backdate(&db, id, 1);
    assert_eq!(due_scheduled_account_ids(&db).unwrap(), vec![acc]);
}

#[test]
fn a_scheduled_send_that_failed_stays_planned_with_a_pause() {
    let (db, acc) = setup();
    let id = schedule(&db, acc, 60);
    backdate(&db, id, 1);
    promote_due(&db, acc).unwrap();
    // Promoted but not yet sent: still due.
    assert_eq!(due_scheduled_account_ids(&db).unwrap(), vec![acc]);
    assert!(claim(&db, id).unwrap());
    mark_failed(&db, id, "421 try later").unwrap();
    // Failed after its time: planned again after the pause, not dropped
    // (an alarm cancelled here would leave it until the next sync) and
    // not due at once (a due timer would spin on it).
    let next = next_due(&db).unwrap().expect("still planned");
    let wait = (next - chrono::Utc::now()).num_seconds();
    assert!(
        wait > SCHEDULED_RETRY_SECS - 30 && wait <= SCHEDULED_RETRY_SECS,
        "{wait}"
    );
    assert!(due_scheduled_account_ids(&db).unwrap().is_empty());
    // Once the pause is over the account is due again.
    backdate(&db, id, SCHEDULED_RETRY_SECS / 60 + 1);
    assert_eq!(due_scheduled_account_ids(&db).unwrap(), vec![acc]);
    // Out of retries: dead, left to the outbox list.
    for _ in 1..MAX_SEND_RETRIES {
        mark_failed(&db, id, "421 try later").unwrap();
    }
    assert_eq!(next_due(&db).unwrap(), None);
}

#[test]
fn the_receipt_flag_survives_promotion() {
    let (db, acc) = setup();
    let to = vec!["b@example.org".to_string()];
    let at = future_ts(60);
    let id = enqueue_scheduled_mime(
        &db,
        acc,
        &ScheduledMime {
            message_id: None,
            raw_mime: RAW,
            envelope_from: "a@example.com",
            envelope_to: &to,
            request_dsn: false,
            notify_self: true,
            send_at: &at,
        },
    )
    .unwrap();
    assert!(get(&db, id).unwrap().notify_self);
    // Backdate it past its time: it promotes into the next flush, and
    // the flag rides along so the receipt still goes out.
    backdate(&db, id, 1);
    assert_eq!(promote_due(&db, acc).unwrap(), 1);
    let row = get(&db, id).unwrap();
    assert_eq!(row.status, QueueStatus::Queued);
    assert!(row.notify_self);
}
