//! What the notification buttons do: Mark read, Archive or Delete, and
//! Reply. Each changes the cache only and hands back what is still pending,
//! for the host to plan the notifications again; the network part (flag and
//! move push, SMTP) is the host's to schedule.

use std::collections::HashMap;

use super::super::{cached_total_unread, collect_pending};
use super::{BackgroundReport, ReadTarget};
use crate::compose::{self, AnswerMode, ComposeForm, PreparedSend};
use crate::db::Db;
use crate::error::Result;
use crate::store::{messages, settings};
use crate::undo::{queue_move_now, MoveTarget, Queued};

/// An "Archive" or "Delete" button (`action` is the notification's
/// `quick_action`): queue the move with no undo window — the notification
/// offers no Undo — and report what is still pending, for re-planning. The
/// mail is hidden at once; the host pushes it (`headless::push_changes`).
/// A delete that would destroy the mail (from Junk or Trash, or with no
/// Trash folder) is refused: that needs the app's confirmation.
pub fn act(
    db: &Db,
    target: &ReadTarget,
    action: &str,
) -> std::result::Result<BackgroundReport, String> {
    let move_to = match settings::normalize_notification_action(action) {
        "trash" => MoveTarget::Trash,
        _ => MoveTarget::Archive,
    };
    for (folder_id, uids) in by_folder(target) {
        match queue_move_now(db, target.account_id, folder_id, &uids, move_to.clone())? {
            Queued::Pending { .. } | Queued::AlreadyThere => {}
            Queued::Permanent => {
                return Err(
                    "this would delete the mail permanently; open the app to delete it".into(),
                )
            }
        }
    }
    Ok(pending_report(db))
}

/// A "Reply" button's text: the reply the composer would start with —
/// recipients, `Re:` subject, signature and quote by the user's settings —
/// with `text` in place of the empty line, validated and queued in the
/// outbox like a composer send. The mail is marked read: it was answered.
/// The adapter delivers the result (`compose::deliver`) on its network
/// thread.
pub fn quick_reply(
    db: &Db,
    target: &ReadTarget,
    text: &str,
) -> std::result::Result<PreparedSend, String> {
    let [mail] = target.mails.as_slice() else {
        return Err("a reply answers one mail".into());
    };
    if text.trim().is_empty() {
        return Err("the reply is empty".into());
    }
    let draft = compose::answer_draft_for(db, mail.folder_id, mail.uid, AnswerMode::Reply)
        .map_err(|_| "this mail is no longer available".to_string())?;
    // The composer's own form, so recipients split exactly as a Send does.
    let form = ComposeForm::parse(
        &serde_json::json!({
            "from": draft.from,
            "to": draft.to,
            "cc": draft.cc,
            "subject": draft.subject,
            "body": draft.with_text(text),
        })
        .to_string(),
    )?;
    let prepared = compose::prepare_send(db, target.account_id, form)?;
    if let Err(e) = messages::set_read_many_by_uids(db, mail.folder_id, &[mail.uid], true) {
        log::warn!("quick reply: could not mark the answered mail read: {e}");
    }
    Ok(prepared)
}

fn by_folder(target: &ReadTarget) -> HashMap<i64, Vec<u32>> {
    let mut by_folder: HashMap<i64, Vec<u32>> = HashMap::new();
    for m in &target.mails {
        by_folder.entry(m.folder_id).or_default().push(m.uid);
    }
    by_folder
}

/// What is still pending after a button changed the cache.
fn pending_report(db: &Db) -> BackgroundReport {
    BackgroundReport {
        pending: collect_pending(db),
        total_unread: cached_total_unread(db),
        ..Default::default()
    }
}

/// A "Mark read" button was pressed: mark `target` read in the cache (queued
/// for the server like any toggle, `flags_dirty`) and report what is still
/// pending, for re-planning the notifications. No network.
pub fn mark_read(db: &Db, target: &ReadTarget) -> Result<BackgroundReport> {
    for (folder_id, uids) in by_folder(target) {
        messages::set_read_many_by_uids(db, folder_id, &uids, true)?;
    }
    Ok(pending_report(db))
}
