//! What a background check does with the new-mail notifications.
//!
//! Pure decisions over a [`BackgroundReport`]: which notifications to post
//! (with or without a sound), which to remove, and what they say. Mail is
//! shown the Android way — one group per account, a summary plus one child
//! per mail, each with a "Mark read" button ([`ReadTarget`]). The Android
//! host only reads back what is on screen, posts and cancels what the plan
//! says and then commits the marks — every scheduler (worker, alarm, push)
//! shares this, so their notifications cannot drift apart.

use std::collections::{HashMap, HashSet};

use chrono::DateTime;
use serde::{Deserialize, Serialize};

use super::{collect_pending, BackgroundReport, NewMail, SeenMark};
use crate::db::Db;
use crate::error::Result;
use crate::store::{account_settings, messages, settings};

/// Payload prefix for "open this message" taps: `mail:<account>:<folder>:<uid>`.
/// A mail's notification carries the same string as its tag.
pub const OPEN_PAYLOAD_PREFIX: &str = "mail:";

/// Tag prefix of an account's group summary: `account:<account>`.
pub const SUMMARY_TAG_PREFIX: &str = "account:";

/// How many mails a summary lists as inbox lines (pre-Android 7 and
/// launchers that show the summary instead of the group).
const SUMMARY_LINES: usize = 5;

/// How many mails of one account get a notification of their own, newest
/// first; the summary still counts and marks them all. Android drops an
/// app's notifications past a few dozen.
const MAX_CHILDREN: usize = 8;

/// What the plan amounts to, for the run history and the host's callbacks.
/// The host does not branch on it to post: `post` and `cancel` say it all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotifyAction {
    /// New mail: post it, and the account's summary makes a sound.
    Alert,
    /// Nothing new, but what is on screen changed (mail was read meanwhile):
    /// repost or remove quietly.
    Update,
    /// Everything on screen has been read: remove it all.
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
    /// Notifications to post or repost, each group's summary before its
    /// children: Android sheds an app's rapid *updates*, never new posts, so
    /// the summary that rings goes out before a burst of new children.
    pub post: Vec<MailNotification>,
    /// Tags of notifications on screen to remove.
    pub cancel: Vec<String>,
    /// How many mails are on screen once the plan is carried out.
    pub count: usize,
    /// Run-history note for `record_outcome`, if the action is worth one.
    pub outcome: Option<String>,
    pub run: String,
    pub marks: Vec<SeenMark>,
}

/// One notification: a mail (child) or an account's group summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MailNotification {
    /// Unique among the app's notifications: the open payload for a mail,
    /// `account:<id>` for a summary.
    pub tag: String,
    /// Android group key, one per account.
    pub group: String,
    pub summary: bool,
    pub title: String,
    pub body: String,
    /// Expanded text of a mail: subject and snippet.
    pub big_text: Option<String>,
    /// Inbox lines of a summary, newest first.
    pub lines: Vec<String>,
    /// `+N more` under a summary's lines.
    pub summary_text: Option<String>,
    /// The account, shown in the notification header.
    pub account: String,
    /// What the lock screen shows instead of sender and subject.
    pub redacted: String,
    /// Mails listed: 1 for a mail, the group's size for a summary.
    pub count: usize,
    /// Arrival time in epoch milliseconds, for ordering.
    pub when: Option<i64>,
    /// Make a sound. Only summaries alert; their children stay quiet.
    pub alert: bool,
    /// Tap target, see [`open_payload`].
    pub payload: String,
    /// What the "Mark read" button marks, as [`ReadTarget`] JSON.
    pub mark_read: String,
}

impl MailNotification {
    /// What [`signature_of`] reads back from the posted notification.
    #[must_use]
    pub fn signature(&self) -> String {
        signature_of(&self.title, &self.body)
    }
}

/// Signature of a posted notification: title and body, as Android reports
/// them back for an active notification.
#[must_use]
pub fn signature_of(title: &str, body: &str) -> String {
    format!("{title}\n{body}")
}

/// The app's notifications on screen: tag → [`signature_of`].
pub type Shown = HashMap<String, String>;

/// What the host reads back from one posted notification.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Posted {
    /// Title and body as Android reports them; the native app sends this.
    Parts {
        #[serde(default)]
        title: String,
        #[serde(default)]
        body: String,
    },
    /// A signature the host built itself: the retired Flutter host still
    /// sends this, so it keeps working unchanged.
    Signature(String),
}

/// [`Shown`] from what the host read back, tag → [`Posted`]. The native host
/// hands over the raw title and body instead of building the signature
/// itself, so the format lives in [`signature_of`] only: a Kotlin twin had
/// to match it byte for byte or every notification read as changed (E20).
#[must_use]
pub fn shown_of(posted: HashMap<String, Posted>) -> Shown {
    posted
        .into_iter()
        .map(|(tag, p)| {
            let signature = match p {
                Posted::Parts { title, body } => signature_of(&title, &body),
                Posted::Signature(s) => s,
            };
            (tag, signature)
        })
        .collect()
}

/// Mail a "Mark read" button marks: one mail, or a whole account group.
/// Hosts treat it as opaque JSON; [`ReadTarget::account_of`] answers the one
/// thing they need from it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadTarget {
    pub account_id: i64,
    pub mails: Vec<ReadMail>,
}

impl ReadTarget {
    /// The account a `ReadTarget` JSON (from a notification's button)
    /// belongs to, so the host can queue its flag push without knowing the
    /// struct's fields (E22).
    pub fn account_of(json: &str) -> Result<i64> {
        let target: Self = serde_json::from_str(json)?;
        Ok(target.account_id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadMail {
    pub folder_id: i64,
    pub uid: u32,
}

/// Plan the notifications for `report`.
///
/// `shown` is what is on screen (empty when nothing is — never posted, or
/// swiped away); `permitted` whether Android lets the app notify;
/// `foreground` whether the app is on screen. A mail swiped away only comes
/// back when it is new; nothing new never adds a notification.
#[must_use]
pub fn plan(
    report: &BackgroundReport,
    alerts_on: bool,
    permitted: bool,
    foreground: bool,
    shown: &Shown,
) -> NotificationPlan {
    let has_new = !report.new.is_empty();
    let settled = |action, outcome: Option<String>| NotificationPlan {
        action,
        post: Vec::new(),
        cancel: Vec::new(),
        count: 0,
        outcome,
        run: report.run.clone(),
        marks: report.marks.clone(),
    };
    if foreground && has_new {
        return settled(
            NotifyAction::Foreground,
            Some("new mail, app is open".into()),
        );
    }
    if has_new && !permitted {
        return settled(
            NotifyAction::Blocked,
            Some("new mail, notifications blocked".into()),
        );
    }
    let alerting = has_new && alerts_on;
    // Pending covers new; the fallback only guards a report without it.
    let listed = if report.pending.is_empty() {
        &report.new
    } else {
        &report.pending
    };
    let fresh: HashSet<String> = report.new.iter().map(open_payload).collect();
    let on_screen: Vec<&NewMail> = listed
        .iter()
        .filter(|m| {
            let tag = open_payload(m);
            shown.contains_key(&tag) || (alerting && fresh.contains(&tag))
        })
        .collect();

    let mut post = Vec::new();
    let mut keep = HashSet::new();
    for group in by_account(&on_screen) {
        let loud = alerting && group.iter().any(|m| fresh.contains(&open_payload(m)));
        let summary = summary_of(&group, loud);
        if loud || shown.get(&summary.tag) != Some(&summary.signature()) {
            post.push(summary.clone());
        }
        keep.insert(summary.tag);
        for m in &group[group.len().saturating_sub(MAX_CHILDREN)..] {
            let child = child_of(m);
            if fresh.contains(&child.tag) && alerting
                || shown.get(&child.tag) != Some(&child.signature())
            {
                post.push(child.clone());
            }
            keep.insert(child.tag);
        }
    }
    let mut cancel: Vec<String> = shown
        .keys()
        .filter(|tag| !keep.contains(*tag))
        .cloned()
        .collect();
    cancel.sort();

    let count = on_screen.len();
    let (action, outcome) = if alerting {
        (NotifyAction::Alert, Some(format!("notified ({count})")))
    } else if has_new {
        (
            NotifyAction::AlertsOff,
            Some("new mail, alerts are off".into()),
        )
    } else if post.is_empty() && cancel.is_empty() {
        (NotifyAction::None, None)
    } else if count == 0 {
        (NotifyAction::Clear, Some("notification cleared".into()))
    } else {
        (NotifyAction::Update, Some("notification updated".into()))
    };
    NotificationPlan {
        action,
        post,
        cancel,
        count,
        ..settled(action, outcome)
    }
}

/// [`plan`] under each account's own "Show notifications" setting
/// (`store::account_settings`): mail of muted accounts stays out of the
/// notifications, and new mail only from muted accounts plans as alerts off.
/// The marks still cover every account, so muted mail never alerts later.
pub fn plan_for(
    db: &Db,
    report: &BackgroundReport,
    permitted: bool,
    foreground: bool,
    shown: &Shown,
) -> NotificationPlan {
    let mut alerts: HashMap<i64, bool> = HashMap::new();
    let mut audible = |m: &NewMail| {
        *alerts.entry(m.account_id).or_insert_with(|| {
            account_settings::get_bool(db, m.account_id, settings::NOTIFICATIONS_ENABLED)
        })
    };
    let mut heard = report.clone();
    heard.new.retain(&mut audible);
    heard.pending.retain(&mut audible);
    if heard.new.is_empty() && !report.new.is_empty() {
        // Still new mail, so it plans as alerts off rather than nothing.
        heard.new.clone_from(&report.new);
        return plan(&heard, false, permitted, foreground, shown);
    }
    plan(&heard, true, permitted, foreground, shown)
}

/// A "Mark read" button was pressed: mark `target` read in the cache (queued
/// for the server like any toggle, `flags_dirty`) and report what is still
/// pending, for re-planning the notifications. No network.
pub fn mark_read(db: &Db, target: &ReadTarget) -> Result<BackgroundReport> {
    let mut by_folder: HashMap<i64, Vec<u32>> = HashMap::new();
    for m in &target.mails {
        by_folder.entry(m.folder_id).or_default().push(m.uid);
    }
    for (folder_id, uids) in by_folder {
        messages::set_read_many_by_uids(db, folder_id, &uids, true)?;
    }
    Ok(BackgroundReport {
        pending: collect_pending(db),
        total_unread: super::cached_total_unread(db),
        ..Default::default()
    })
}

/// `items` grouped by account, in order of first appearance; each group
/// keeps report order (oldest first).
fn by_account<'a>(items: &[&'a NewMail]) -> Vec<Vec<&'a NewMail>> {
    let mut groups: Vec<Vec<&NewMail>> = Vec::new();
    for m in items {
        match groups.iter_mut().find(|g| g[0].account_id == m.account_id) {
            Some(g) => g.push(m),
            None => groups.push(vec![m]),
        }
    }
    groups
}

/// The notification for one mail.
#[must_use]
pub fn child_of(m: &NewMail) -> MailNotification {
    let subject = body_of(m);
    let snippet = m.snippet.trim();
    MailNotification {
        tag: open_payload(m),
        group: group_of(m.account_id),
        summary: false,
        title: title_of(m),
        body: subject.clone(),
        big_text: Some(if snippet.is_empty() {
            subject
        } else {
            format!("{subject}\n{snippet}")
        }),
        lines: Vec::new(),
        summary_text: None,
        account: m.account_email.trim().to_string(),
        redacted: "New message".into(),
        count: 1,
        when: when_of(m),
        alert: false,
        payload: open_payload(m),
        mark_read: read_target(m.account_id, &[m]),
    }
}

/// The summary of one account's group (`group` never empty, oldest first):
/// counts the mails, lists the newest, opens the newest.
#[must_use]
pub fn summary_of(group: &[&NewMail], alert: bool) -> MailNotification {
    let newest = group[group.len() - 1];
    let n = group.len();
    let title = if n == 1 {
        "1 new message".to_string()
    } else {
        format!("{n} new messages")
    };
    MailNotification {
        tag: format!("{SUMMARY_TAG_PREFIX}{}", newest.account_id),
        group: group_of(newest.account_id),
        summary: true,
        title: title.clone(),
        body: line_of(newest),
        big_text: None,
        lines: group
            .iter()
            .rev()
            .take(SUMMARY_LINES)
            .map(|m| line_of(m))
            .collect(),
        summary_text: (n > SUMMARY_LINES).then(|| format!("+{} more", n - SUMMARY_LINES)),
        account: newest.account_email.trim().to_string(),
        redacted: title,
        count: n,
        when: when_of(newest),
        alert,
        payload: open_payload(newest),
        mark_read: read_target(newest.account_id, group),
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

fn group_of(account_id: i64) -> String {
    format!("mail-account-{account_id}")
}

fn read_target(account_id: i64, mails: &[&NewMail]) -> String {
    let target = ReadTarget {
        account_id,
        mails: mails
            .iter()
            .map(|m| ReadMail {
                folder_id: m.folder_id,
                uid: m.uid,
            })
            .collect(),
    };
    serde_json::to_string(&target).unwrap_or_default()
}

fn when_of(m: &NewMail) -> Option<i64> {
    DateTime::parse_from_rfc3339(m.date.trim())
        .ok()
        .map(|d| d.timestamp_millis())
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
mod tests;
