//! What a background check does with the new-mail notification.
//!
//! Pure decisions over a [`BackgroundReport`]: whether to alert, quietly
//! update, clear or leave the one notification alone, and what it says.
//! The Android host only reads back what is on screen, posts the plan and
//! then commits the marks — every scheduler (worker, alarm, push) shares
//! this, so their notifications cannot drift apart.

use serde::Serialize;

use super::{BackgroundReport, NewMail, SeenMark};

/// Payload prefix for "open this message" taps: `mail:<account>:<folder>:<uid>`.
pub const OPEN_PAYLOAD_PREFIX: &str = "mail:";

/// What to do with the notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyAction {
    /// New mail: post (or replace) the notification and make a sound.
    Alert,
    /// Nothing new, but the visible notification lists mail that has been
    /// read meanwhile: repost it quietly.
    Update,
    /// Everything the visible notification listed has been read: remove it.
    Clear,
    /// New mail, but the user turned alerts off.
    AlertsOff,
    /// New mail, but Android does not let the app notify.
    Blocked,
    /// New mail while the app is on screen: the list shows it already.
    Foreground,
    /// Nothing to do.
    None,
}

/// What the host should do after a check, and what it should commit once
/// done. `marks` go back unchanged to `commit_seen` only after the post
/// succeeded, so a failed post reports the same mail again next run.
#[derive(Debug, Clone, Serialize)]
pub struct NotificationPlan {
    pub action: NotifyAction,
    pub title: String,
    pub body: String,
    /// Inbox-style lines, newest first; empty for a single mail.
    pub lines: Vec<String>,
    pub summary: Option<String>,
    /// How many mails the notification lists.
    pub count: usize,
    /// Tap target, see [`open_payload`]; empty when nothing is listed.
    pub payload: String,
    /// Run-history note for `record_outcome`, if the action is worth one.
    pub outcome: Option<String>,
    pub run: String,
    pub marks: Vec<SeenMark>,
}

/// Title, body and inbox lines of the notification for some mail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationText {
    pub title: String,
    pub body: String,
    pub lines: Vec<String>,
    pub summary: Option<String>,
}

impl NotificationText {
    /// What [`signature_of`] reads back from the posted notification.
    #[must_use]
    pub fn signature(&self) -> String {
        signature_of(&self.title, &self.body)
    }
}

/// Signature of a posted notification: title and body, as Android reports
/// them back for the active notification.
#[must_use]
pub fn signature_of(title: &str, body: &str) -> String {
    format!("{title}\n{body}")
}

/// Plan the notification for `report`.
///
/// `shown` is the signature of the notification on screen (`None` when there
/// is none — never posted, or swiped away); `permitted` whether Android lets
/// the app notify; `foreground` whether the app is on screen.
#[must_use]
pub fn plan(
    report: &BackgroundReport,
    alerts_on: bool,
    permitted: bool,
    foreground: bool,
    shown: Option<&str>,
) -> NotificationPlan {
    // Pending covers new; the fallback only guards a report without it.
    let listed = if report.pending.is_empty() {
        &report.new
    } else {
        &report.pending
    };
    let text = (!listed.is_empty()).then(|| notification_text(listed));
    let action = if foreground && !report.new.is_empty() {
        NotifyAction::Foreground
    } else {
        decide(
            !report.new.is_empty(),
            alerts_on,
            permitted,
            shown,
            text.as_ref().map(NotificationText::signature).as_deref(),
        )
    };
    let outcome = match action {
        NotifyAction::Alert => Some(format!("notified ({})", listed.len())),
        NotifyAction::Update => Some("notification updated".to_string()),
        NotifyAction::Clear => Some("notification cleared".to_string()),
        NotifyAction::AlertsOff => Some("new mail, alerts are off".to_string()),
        NotifyAction::Blocked => Some("new mail, notifications blocked".to_string()),
        NotifyAction::Foreground => Some("new mail, app is open".to_string()),
        NotifyAction::None => None,
    };
    let text = text.unwrap_or(NotificationText {
        title: String::new(),
        body: String::new(),
        lines: Vec::new(),
        summary: None,
    });
    NotificationPlan {
        action,
        title: text.title,
        body: text.body,
        lines: text.lines,
        summary: text.summary,
        count: listed.len(),
        payload: listed.last().map(open_payload).unwrap_or_default(),
        outcome,
        run: report.run.clone(),
        marks: report.marks.clone(),
    }
}

/// The alert decision. `wanted` is the signature of what the notification
/// should list now (`None` when nothing is unseen). A swiped-away
/// notification only comes back for new mail.
#[must_use]
pub fn decide(
    has_new: bool,
    alerts_on: bool,
    permitted: bool,
    shown: Option<&str>,
    wanted: Option<&str>,
) -> NotifyAction {
    if has_new {
        if !alerts_on {
            return NotifyAction::AlertsOff;
        }
        return if permitted {
            NotifyAction::Alert
        } else {
            NotifyAction::Blocked
        };
    }
    match (shown, wanted) {
        (None, _) => NotifyAction::None,
        (Some(_), None) => NotifyAction::Clear,
        (Some(s), Some(w)) if s == w => NotifyAction::None,
        _ => NotifyAction::Update,
    }
}

/// Layout for `items` (never empty), in report order (oldest first): the
/// title names the sender of the newest, or the count when there are
/// several; lines list the newest five.
#[must_use]
pub fn notification_text(items: &[NewMail]) -> NotificationText {
    let newest = &items[items.len() - 1];
    if items.len() == 1 {
        return NotificationText {
            title: title_of(newest),
            body: body_of(newest),
            lines: Vec::new(),
            summary: None,
        };
    }
    NotificationText {
        title: format!("{} new messages", items.len()),
        body: line_of(newest),
        lines: items.iter().rev().take(5).map(line_of).collect(),
        summary: (items.len() > 5).then(|| format!("+{} more", items.len() - 5)),
    }
}

/// Tap target of a notification for `mail`: `mail:<account>:<folder>:<uid>`.
#[must_use]
pub fn open_payload(mail: &NewMail) -> String {
    format!(
        "{OPEN_PAYLOAD_PREFIX}{}:{}:{}",
        mail.account_id, mail.folder_id, mail.uid
    )
}

fn title_of(m: &NewMail) -> String {
    let from = m.from.trim();
    if from.is_empty() {
        m.account_email.trim().to_string()
    } else {
        from.to_string()
    }
}

fn body_of(m: &NewMail) -> String {
    let subject = m.subject.trim();
    if subject.is_empty() {
        "(no subject)".to_string()
    } else {
        subject.to_string()
    }
}

fn line_of(m: &NewMail) -> String {
    format!("{} — {}", title_of(m), body_of(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mail(uid: u32, from: &str) -> NewMail {
        NewMail {
            account_id: 1,
            folder_id: 4,
            uid,
            from: from.to_string(),
            subject: format!("subject {uid}"),
            ..Default::default()
        }
    }

    #[test]
    fn new_mail_alerts_unless_alerts_are_off_or_blocked() {
        assert_eq!(
            decide(true, true, true, None, Some("a")),
            NotifyAction::Alert
        );
        assert_eq!(
            decide(true, true, true, Some("a"), Some("a")),
            NotifyAction::Alert
        );
        assert_eq!(
            decide(true, false, true, None, None),
            NotifyAction::AlertsOff
        );
        assert_eq!(decide(true, true, false, None, None), NotifyAction::Blocked);
    }

    #[test]
    fn nothing_new_leaves_a_dismissed_notification_alone() {
        assert_eq!(
            decide(false, true, true, None, Some("a")),
            NotifyAction::None
        );
        assert_eq!(decide(false, true, true, None, None), NotifyAction::None);
    }

    #[test]
    fn nothing_new_keeps_a_visible_notification_in_step() {
        assert_eq!(
            decide(false, true, true, Some("a"), Some("a")),
            NotifyAction::None
        );
        assert_eq!(
            decide(false, true, true, Some("a"), Some("b")),
            NotifyAction::Update
        );
        assert_eq!(
            decide(false, true, true, Some("a"), None),
            NotifyAction::Clear
        );
    }

    #[test]
    fn one_mail_names_its_sender_and_subject() {
        let t = notification_text(&[mail(1, "a@example.com")]);
        assert_eq!(t.title, "a@example.com");
        assert_eq!(t.body, "subject 1");
        assert!(t.lines.is_empty());
    }

    #[test]
    fn a_mail_without_sender_or_subject_still_reads() {
        let mut m = mail(1, " ");
        m.account_email = "me@example.com".to_string();
        m.subject = String::new();
        let t = notification_text(&[m]);
        assert_eq!(t.title, "me@example.com");
        assert_eq!(t.body, "(no subject)");
    }

    #[test]
    fn several_mails_count_newest_first_capped_at_five_lines() {
        let items: Vec<_> = (1..=7)
            .map(|i| mail(i, &format!("s{i}@example.com")))
            .collect();
        let t = notification_text(&items);
        assert_eq!(t.title, "7 new messages");
        assert_eq!(t.body, "s7@example.com — subject 7");
        assert_eq!(t.lines.len(), 5);
        assert!(t.lines[0].starts_with("s7@"));
        assert_eq!(t.summary.as_deref(), Some("+2 more"));
    }

    #[test]
    fn the_plan_opens_the_newest_and_carries_marks_and_run() {
        let report = BackgroundReport {
            new: vec![mail(9, "b@example.org")],
            pending: vec![mail(8, "a@example.org"), mail(9, "b@example.org")],
            marks: vec![SeenMark {
                account_id: 1,
                folder_id: 4,
                uid_validity: 7,
                uid: 9,
            }],
            run: "started".to_string(),
            ..Default::default()
        };
        let p = plan(&report, true, true, false, None);
        assert_eq!(p.action, NotifyAction::Alert);
        assert_eq!(p.count, 2);
        assert_eq!(p.payload, "mail:1:4:9");
        assert_eq!(p.title, "2 new messages");
        assert_eq!(p.outcome.as_deref(), Some("notified (2)"));
        assert_eq!(p.run, "started");
        assert_eq!(p.marks, report.marks);
    }

    #[test]
    fn the_open_app_takes_new_mail_without_an_alert() {
        let report = BackgroundReport {
            new: vec![mail(9, "b@example.org")],
            ..Default::default()
        };
        let p = plan(&report, true, true, true, None);
        assert_eq!(p.action, NotifyAction::Foreground);
    }

    #[test]
    fn a_visible_notification_follows_what_is_still_unread() {
        let listed = vec![mail(8, "a@example.org")];
        let shown = notification_text(&[mail(8, "a@example.org"), mail(9, "b@example.org")]);
        let report = BackgroundReport {
            pending: listed.clone(),
            ..Default::default()
        };
        let p = plan(&report, true, true, false, Some(&shown.signature()));
        assert_eq!(p.action, NotifyAction::Update);
        assert_eq!(p.title, "a@example.org");

        let p = plan(
            &BackgroundReport::default(),
            true,
            true,
            false,
            Some(&shown.signature()),
        );
        assert_eq!(p.action, NotifyAction::Clear);
        assert!(p.payload.is_empty());
    }
}
