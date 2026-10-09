use super::*;
use crate::sync::background::{collect_new_mail, commit_seen};
use crate::sync::headless::tests::setup_db;

fn mail(uid: u32, from: &str) -> NewMail {
    NewMail {
        account_id: 1,
        account_email: "me@example.com".to_string(),
        folder_id: 4,
        uid,
        from: from.to_string(),
        subject: format!("subject {uid}"),
        ..Default::default()
    }
}

fn of_account(account_id: i64, uid: u32) -> NewMail {
    NewMail {
        account_id,
        account_email: format!("acc{account_id}@example.com"),
        ..mail(uid, "c@example.org")
    }
}

fn arrived(new: Vec<NewMail>, pending: Vec<NewMail>) -> BackgroundReport {
    BackgroundReport {
        new,
        pending,
        run: "started".to_string(),
        ..Default::default()
    }
}

/// What Android reports back after carrying out `plan` over `before`.
fn on_screen(before: &Shown, plan: &NotificationPlan) -> Shown {
    let mut shown = before.clone();
    for tag in &plan.cancel {
        shown.remove(tag);
    }
    for n in &plan.post {
        shown.insert(n.tag.clone(), n.signature());
    }
    shown
}

fn tags(plan: &NotificationPlan) -> Vec<&str> {
    plan.post.iter().map(|n| n.tag.as_str()).collect()
}

#[test]
fn new_mail_posts_a_child_each_and_a_loud_summary() {
    let both = vec![mail(8, "a@example.org"), mail(9, "b@example.org")];
    let p = plan(
        &arrived(both.clone(), both),
        true,
        true,
        false,
        &Shown::new(),
    );
    assert_eq!(p.action, NotifyAction::Alert);
    assert_eq!(tags(&p), ["account:1", "mail:1:4:8", "mail:1:4:9"]);
    assert!(p.post[1..].iter().all(|n| !n.summary && !n.alert));
    let summary = &p.post[0];
    assert!(summary.summary && summary.alert);
    assert_eq!(summary.title, "2 new messages");
    assert_eq!(
        summary.payload, "mail:1:4:9",
        "the summary opens the newest"
    );
    assert!(summary.lines[0].starts_with("b@"));
    assert_eq!(p.count, 2);
    assert_eq!(p.outcome.as_deref(), Some("notified (2)"));
    assert_eq!(p.run, "started");
}

#[test]
fn each_account_gets_its_own_group_and_only_new_mail_rings() {
    let old = of_account(1, 1);
    let shown = on_screen(
        &Shown::new(),
        &plan(
            &arrived(vec![old.clone()], vec![old.clone()]),
            true,
            true,
            false,
            &Shown::new(),
        ),
    );
    let new = of_account(2, 5);
    let p = plan(
        &arrived(vec![new.clone()], vec![old, new]),
        true,
        true,
        false,
        &shown,
    );
    assert_eq!(
        tags(&p),
        ["account:2", "mail:2:4:5"],
        "account 1 is unchanged"
    );
    assert_eq!(p.post[0].group, "mail-account-2");
    assert!(p.post[0].alert);
    assert_eq!(p.post[0].account, "acc2@example.com");
    assert!(p.cancel.is_empty());
    assert_eq!(p.count, 2);
}

#[test]
fn a_mail_swiped_away_only_comes_back_when_new() {
    let a = mail(8, "a@example.org");
    let b = mail(9, "b@example.org");
    // a was notified earlier and swiped away; b arrives.
    let p = plan(
        &arrived(vec![b.clone()], vec![a.clone(), b.clone()]),
        true,
        true,
        false,
        &Shown::new(),
    );
    assert_eq!(tags(&p), ["account:1", "mail:1:4:9"]);
    assert_eq!(p.post[0].title, "1 new message");

    let p = plan(
        &arrived(vec![], vec![a, b]),
        true,
        true,
        false,
        &Shown::new(),
    );
    assert_eq!(p.action, NotifyAction::None);
    assert!(p.post.is_empty());
}

#[test]
fn reading_mail_removes_its_child_and_quietly_recounts_the_summary() {
    let all = vec![mail(7, "a@example.org"), mail(8, "b@example.org")];
    let shown = on_screen(
        &Shown::new(),
        &plan(&arrived(all.clone(), all), true, true, false, &Shown::new()),
    );
    let p = plan(
        &arrived(vec![], vec![mail(8, "b@example.org")]),
        true,
        true,
        false,
        &shown,
    );
    assert_eq!(p.action, NotifyAction::Update);
    assert_eq!(p.cancel, ["mail:1:4:7"]);
    assert_eq!(tags(&p), ["account:1"]);
    assert!(!p.post[0].alert);
    assert_eq!(p.post[0].title, "1 new message");

    let p = plan(&BackgroundReport::default(), true, true, false, &shown);
    assert_eq!(p.action, NotifyAction::Clear);
    assert_eq!(p.cancel, ["account:1", "mail:1:4:7", "mail:1:4:8"]);
    assert_eq!(p.count, 0);
}

#[test]
fn an_unchanged_screen_plans_nothing() {
    let all = vec![mail(7, "a@example.org")];
    let shown = on_screen(
        &Shown::new(),
        &plan(
            &arrived(all.clone(), all.clone()),
            true,
            true,
            false,
            &Shown::new(),
        ),
    );
    let p = plan(&arrived(vec![], all), true, true, false, &shown);
    assert_eq!(p.action, NotifyAction::None);
    assert!(p.post.is_empty() && p.cancel.is_empty());
    assert_eq!(p.outcome, None);
}

#[test]
fn alerts_off_blocked_and_foreground_post_nothing_new() {
    let one = vec![mail(9, "b@example.org")];
    let report = BackgroundReport {
        marks: vec![SeenMark::default()],
        ..arrived(one.clone(), one)
    };
    let off = plan(&report, false, true, false, &Shown::new());
    assert_eq!(off.action, NotifyAction::AlertsOff);
    assert!(off.post.is_empty());
    let blocked = plan(&report, true, false, false, &Shown::new());
    assert_eq!(blocked.action, NotifyAction::Blocked);
    let open = plan(&report, true, true, true, &Shown::new());
    assert_eq!(open.action, NotifyAction::Foreground);
    for p in [off, blocked, open] {
        assert_eq!(p.marks.len(), 1, "marks are committed either way");
    }
}

#[test]
fn a_burst_gets_children_for_the_newest_only() {
    let items: Vec<_> = (1..=12).map(|i| mail(i, "a@example.org")).collect();
    let p = plan(
        &arrived(items.clone(), items),
        true,
        true,
        false,
        &Shown::new(),
    );
    assert_eq!(p.post.len(), 1 + MAX_CHILDREN);
    assert_eq!(p.post[0].count, 12, "the summary counts them all");
    assert_eq!(p.post[1].tag, "mail:1:4:5");
    let target: ReadTarget = serde_json::from_str(&p.post[0].mark_read).unwrap();
    assert_eq!(target.mails.len(), 12);
}

#[test]
fn a_mail_names_its_sender_and_expands_to_the_snippet() {
    let mut m = mail(1, "a@example.com");
    m.snippet = "  first words ".to_string();
    m.date = "2026-01-02T03:04:05Z".to_string();
    let n = child_of(&m);
    assert_eq!(n.title, "a@example.com");
    assert_eq!(n.body, "subject 1");
    assert_eq!(n.big_text.as_deref(), Some("subject 1\nfirst words"));
    assert_eq!(n.when, Some(1_767_323_045_000));
    let target: ReadTarget = serde_json::from_str(&n.mark_read).unwrap();
    assert_eq!(
        target,
        ReadTarget {
            account_id: 1,
            mails: vec![ReadMail {
                folder_id: 4,
                uid: 1
            }],
        }
    );
}

#[test]
fn a_mail_without_sender_or_subject_still_reads() {
    let mut m = mail(1, " ");
    m.subject = String::new();
    let n = child_of(&m);
    assert_eq!(n.title, "me@example.com");
    assert_eq!(n.body, "(no subject)");
    assert_eq!(n.big_text.as_deref(), Some("(no subject)"));
    assert_eq!(n.when, None);
}

#[test]
fn a_summary_lists_the_newest_five_and_marks_the_whole_group() {
    let items: Vec<_> = (1..=7)
        .map(|i| mail(i, &format!("s{i}@example.com")))
        .collect();
    let refs: Vec<&NewMail> = items.iter().collect();
    let s = summary_of(&refs, false);
    assert_eq!(s.title, "7 new messages");
    assert_eq!(s.body, "s7@example.com — subject 7");
    assert_eq!(s.lines.len(), 5);
    assert!(s.lines[0].starts_with("s7@"));
    assert_eq!(s.summary_text.as_deref(), Some("+2 more"));
    let target: ReadTarget = serde_json::from_str(&s.mark_read).unwrap();
    assert_eq!(target.mails.len(), 7);
}

#[test]
fn muted_accounts_stay_out_of_the_notifications() {
    let db = Db::open_in_memory().unwrap();
    let loud = crate::store::accounts::create_for_test(&db, "a@example.com");
    let muted = crate::store::accounts::create_for_test(&db, "b@example.org");
    account_settings::set_overrides(
        &db,
        muted,
        &[(settings::NOTIFICATIONS_ENABLED.to_string(), "0".to_string())],
    )
    .unwrap();
    let both = vec![of_account(loud, 1), of_account(muted, 2)];
    let p = plan_for(
        &db,
        &arrived(both.clone(), both),
        true,
        false,
        &Shown::new(),
    );
    assert_eq!(p.action, NotifyAction::Alert);
    assert_eq!(p.count, 1);
    assert_eq!(p.post[1].tag, open_payload(&of_account(loud, 1)));

    let only_muted = BackgroundReport {
        marks: vec![SeenMark::default()],
        ..arrived(vec![of_account(muted, 3)], vec![])
    };
    let p = plan_for(&db, &only_muted, true, false, &Shown::new());
    assert_eq!(p.action, NotifyAction::AlertsOff);
    assert!(p.post.is_empty());
    assert_eq!(p.marks.len(), 1, "muted mail is still marked seen");
}

#[test]
fn mark_read_takes_the_mail_out_of_what_is_pending() {
    let (db, acc, inbox, _) = setup_db();
    commit_seen(&db, &collect_new_mail(&db).1);
    for uid in [3, 4] {
        let mut m = messages::sample_new(acc, inbox, uid);
        m.is_read = false;
        messages::upsert(&db, &m).unwrap();
    }
    commit_seen(&db, &collect_new_mail(&db).1);
    assert_eq!(collect_pending(&db).len(), 2);

    let report = mark_read(
        &db,
        &ReadTarget {
            account_id: acc,
            mails: vec![ReadMail {
                folder_id: inbox,
                uid: 3,
            }],
        },
    )
    .unwrap();
    assert!(report.new.is_empty() && report.marks.is_empty());
    assert_eq!(
        report.pending.iter().map(|m| m.uid).collect::<Vec<_>>(),
        [4]
    );
    let dirty = messages::list_flags_dirty(&db, acc).unwrap();
    assert_eq!(dirty.len(), 1, "queued for the server");
    assert!(dirty[0].is_read);
}

#[test]
fn shown_is_built_from_what_the_host_reads_back() {
    // E20: the host sends title and body; the signature format stays here.
    let posted: HashMap<String, Posted> = serde_json::from_str(
        r#"{"mail:1:2:3": {"title": "Ann", "body": "Hello"}, "account:1": {"title": "2 new"},
            "mail:1:2:4": "Bob\nHi"}"#,
    )
    .unwrap();
    let shown = shown_of(posted);
    assert_eq!(shown["mail:1:2:3"], signature_of("Ann", "Hello"));
    assert_eq!(shown["account:1"], signature_of("2 new", ""));
    // The Flutter host's pre-built signature passes through as it was.
    assert_eq!(shown["mail:1:2:4"], signature_of("Bob", "Hi"));
}

#[test]
fn a_read_target_names_its_account() {
    let json = serde_json::to_string(&ReadTarget {
        account_id: 7,
        mails: vec![ReadMail {
            folder_id: 2,
            uid: 3,
        }],
    })
    .unwrap();
    assert_eq!(ReadTarget::account_of(&json).unwrap(), 7);
    assert!(ReadTarget::account_of("not json").is_err());
}
