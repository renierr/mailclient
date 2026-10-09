//! Undoable delete, archive and move, shared by both frontends.
//!
//! An action does not touch the server at once. The messages are queued in
//! `pending_moves` — which hides them from every list immediately — and the
//! IMAP move runs once [`UNDO_GRACE_SECS`] have passed, from
//! [`push_local_changes`] or any sync. Undo within that window deletes the
//! queue rows and the messages are simply back; nothing ever left the server.
//!
//! Only reversible actions go this way. A delete that destroys (from Junk,
//! from Trash, or with no Trash folder) is reported as
//! [`Queued::Permanent`]: the caller confirms and purges as before.

use crate::models::{Folder, FolderRole};
use crate::store::pending_moves::{self, PendingAction};
use crate::store::{folders, messages};
use crate::sync::pool::{checkout_session, job_account};
use crate::Db;

mod grace;
pub use grace::{push_after_grace, Due};

/// How long an action stays undoable before it is pushed to the server.
pub const UNDO_GRACE_SECS: i64 = 8;

/// Where an undoable action sends the messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveTarget {
    /// "Delete": the account's Trash.
    Trash,
    /// The account's Archive (created on push when missing).
    Archive,
    /// Any folder of the same account, by path.
    Folder(String),
}

/// Outcome of [`queue_move`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Queued {
    /// Hidden now, pushed after the grace period. `batch` is the Undo handle.
    Pending {
        batch: String,
        count: u64,
        label: String,
    },
    /// This delete cannot go to Trash; it destroys. Not queued.
    Permanent,
    /// Source and target are the same folder; nothing to do.
    AlreadyThere,
}

/// Whether deleting from `src` destroys instead of moving to Trash: from
/// Junk (spam never touches Trash), from Trash itself, or when the account
/// has no Trash folder. `folders` is the account's folder list. Both
/// frontends' confirm dialogs say which, because only one is undoable.
#[must_use]
pub fn delete_is_permanent(src: &Folder, folders: &[Folder]) -> bool {
    src.role == FolderRole::Junk
        || !folders
            .iter()
            .any(|f| f.role == FolderRole::Trash && f.id != src.id)
}

/// What a delete is about to do, and whether the user is asked first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct DeletePrompt {
    /// At least one target is destroyed rather than moved to Trash.
    pub permanent: bool,
    /// Show the confirm dialog before deleting.
    pub ask: bool,
}

/// The one delete-confirm rule for every frontend. `permanent` holds each
/// target's folder `delete_is_permanent` (`None`: folder not known, which
/// counts as destroying, so the dialog never under-warns). A delete
/// destroys when any target would — a search selection can span Inbox and
/// Trash. It asks when it destroys (no undo), for every bulk delete, and
/// otherwise when the `confirm_delete` preference is on.
#[must_use]
pub fn delete_prompt(confirm_pref: bool, bulk: bool, permanent: &[Option<bool>]) -> DeletePrompt {
    let permanent = permanent.is_empty() || permanent.iter().any(|p| *p != Some(false));
    DeletePrompt {
        permanent,
        ask: permanent || bulk || confirm_pref,
    }
}

/// Queue an undoable action on `uids` of `folder_id`.
pub fn queue_move(
    db: &Db,
    account_id: i64,
    folder_id: i64,
    uids: &[u32],
    target: MoveTarget,
) -> Result<Queued, String> {
    let batch = uuid::Uuid::new_v4().to_string();
    Ok(
        match queue_into(db, account_id, folder_id, uids, &target, &batch)? {
            Share::Pending {
                count,
                action,
                place,
            } => Queued::Pending {
                batch,
                count,
                label: label(action, count, &place),
            },
            Share::Permanent => Queued::Permanent,
            Share::AlreadyThere => Queued::AlreadyThere,
        },
    )
}

/// What one folder's share of a queued action came to.
pub(crate) enum Share {
    Pending {
        count: u64,
        action: PendingAction,
        place: String,
    },
    Permanent,
    AlreadyThere,
}

/// The Undo toast's text.
pub(crate) fn label(action: PendingAction, count: u64, place: &str) -> String {
    match action {
        PendingAction::Archive => format!("Archived {count} to {place}"),
        PendingAction::Trash | PendingAction::Move => format!("Moved {count} to {place}"),
    }
}

/// [`queue_move`] into an existing `batch`, so one Undo can take back an
/// action that spans folders (see [`crate::bulk`]).
pub(crate) fn queue_into(
    db: &Db,
    account_id: i64,
    folder_id: i64,
    uids: &[u32],
    target: &MoveTarget,
    batch: &str,
) -> Result<Share, String> {
    if uids.is_empty() {
        return Err("no messages selected".to_string());
    }
    let src = folders::get(db, folder_id).map_err(|_| "unknown folder".to_string())?;
    if src.account_id != account_id {
        return Err("folder does not belong to this account".to_string());
    }
    let known = folders::list_by_account(db, account_id).map_err(|e| e.to_string())?;
    let (action, dest_id, place) = match target {
        MoveTarget::Trash => {
            if delete_is_permanent(&src, &known) {
                return Ok(Share::Permanent);
            }
            let trash = known
                .iter()
                .find(|f| f.role == FolderRole::Trash && f.id != src.id)
                .map_or_else(String::new, |t| t.path.clone());
            (PendingAction::Trash, None, trash)
        }
        MoveTarget::Archive => match known.iter().find(|f| f.role == FolderRole::Archive) {
            Some(a) if a.id == src.id => return Ok(Share::AlreadyThere),
            Some(a) => (PendingAction::Archive, Some(a.id), a.path.clone()),
            None => (PendingAction::Archive, None, "Archive".to_string()),
        },
        MoveTarget::Folder(path) => {
            let dest = known
                .iter()
                .find(|f| &f.path == path)
                .ok_or_else(|| format!("unknown folder {path}"))?;
            if dest.id == src.id {
                return Ok(Share::AlreadyThere);
            }
            (PendingAction::Move, Some(dest.id), dest.path.clone())
        }
    };
    let ids = messages::ids_by_uids(db, folder_id, uids).map_err(|e| e.to_string())?;
    if ids.is_empty() {
        return Err("message is no longer available".to_string());
    }
    let due = (chrono::Utc::now() + chrono::Duration::seconds(UNDO_GRACE_SECS))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let count =
        pending_moves::queue(db, &ids, action, dest_id, batch, &due).map_err(|e| e.to_string())?;
    Ok(Share::Pending {
        count,
        action,
        place,
    })
}

/// Take back a queued action. Returns how many messages came back; 0 means
/// it was already pushed and can no longer be undone this way.
pub fn undo(db: &Db, batch: &str) -> Result<u64, String> {
    pending_moves::cancel_batch(db, batch).map_err(|e| e.to_string())
}

/// What [`push_local_changes`] pushed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Pushed {
    pub flags: u64,
    pub moves: u64,
}

/// Push the account's local changes — read/star toggles and due moves —
/// over a pooled session. Quiet by design: nothing to push means no network
/// at all, and offline or failing pushes stay queued for the next sync.
pub async fn push_local_changes(db: &Db, account_id: i64) -> Pushed {
    let flags_dirty = !messages::list_flags_dirty(db, account_id)
        .unwrap_or_default()
        .is_empty();
    let now = crate::store::now();
    let moves_due = !pending_moves::list_due(db, account_id, &now)
        .unwrap_or_default()
        .is_empty();
    if !flags_dirty && !moves_due {
        return Pushed::default();
    }
    let acc = match job_account(db, account_id) {
        Ok(a) => a,
        Err(e) => {
            log::debug!("local-push: {e}");
            return Pushed::default();
        }
    };
    let mut imap = match checkout_session(&acc).await {
        Ok(l) => l,
        Err(e) => {
            log::debug!("local-push: offline, staying queued: {e}");
            return Pushed::default();
        }
    };
    // Flags first: a toggle on a message that is about to move must reach
    // the server while its UID is still valid in the source folder.
    let pushed = Pushed {
        flags: imap.push_dirty_flags(db, acc.id).await,
        moves: imap.push_due_moves(db, acc.id).await,
    };
    imap.checkin();
    if pushed.flags + pushed.moves > 0 {
        log::info!(
            "local-push: {} flag change(s), {} move(s)",
            pushed.flags,
            pushed.moves
        );
    }
    pushed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAccount;
    use crate::store::accounts;

    #[test]
    fn delete_prompt_destroys_when_any_target_does() {
        let p = delete_prompt(false, true, &[Some(false), Some(true)]);
        assert!(p.permanent && p.ask);
        let p = delete_prompt(false, true, &[Some(false), None]);
        assert!(p.permanent, "an unknown folder counts as destroying");
        assert!(delete_prompt(false, false, &[]).permanent);
    }

    #[test]
    fn delete_prompt_asks_for_permanent_bulk_or_preference() {
        let trash = [Some(false)];
        assert!(!delete_prompt(false, false, &trash).ask);
        assert!(delete_prompt(true, false, &trash).ask);
        assert!(delete_prompt(false, true, &trash).ask);
        let p = delete_prompt(false, false, &[Some(true)]);
        assert!(p.permanent && p.ask, "no undo, so it always asks");
        assert!(!delete_prompt(true, false, &trash).permanent);
    }

    struct Fx {
        db: Db,
        acc: i64,
        inbox: i64,
        trash: i64,
        junk: i64,
        work: i64,
    }

    fn fx(with_trash: bool) -> Fx {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "t".to_string(),
                email_address: "me@example.com".to_string(),
                from_name: String::new(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                smtp_security: "starttls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 300,
            },
        )
        .unwrap();
        let inbox = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        let trash = if with_trash {
            folders::upsert(&db, acc, "Trash", "/", FolderRole::Trash).unwrap()
        } else {
            -1
        };
        let junk = folders::upsert(&db, acc, "Junk", "/", FolderRole::Junk).unwrap();
        let work = folders::upsert(&db, acc, "Work", "/", FolderRole::Custom).unwrap();
        for (folder, uid) in [(inbox, 1), (inbox, 2), (junk, 3)] {
            messages::upsert(&db, &messages::sample_new(acc, folder, uid)).unwrap();
        }
        Fx {
            db,
            acc,
            inbox,
            trash,
            junk,
            work,
        }
    }

    fn visible(f: &Fx, folder: i64) -> u64 {
        messages::count_by_folder(&f.db, folder).unwrap()
    }

    #[test]
    fn delete_hides_at_once_and_undo_brings_it_back() {
        let f = fx(true);
        let q = queue_move(&f.db, f.acc, f.inbox, &[1, 2], MoveTarget::Trash).unwrap();
        let Queued::Pending {
            batch,
            count,
            label,
        } = q
        else {
            panic!("expected pending, got {q:?}");
        };
        assert_eq!(count, 2);
        assert_eq!(label, "Moved 2 to Trash");
        assert_eq!(visible(&f, f.inbox), 0);
        assert_eq!(
            messages::counts_by_account(&f.db, f.acc)
                .unwrap()
                .get(&f.inbox)
                .map(|c| c.total)
                .unwrap_or(0),
            0
        );
        assert_eq!(undo(&f.db, &batch).unwrap(), 2);
        assert_eq!(visible(&f, f.inbox), 2);
        assert_eq!(undo(&f.db, &batch).unwrap(), 0);
    }

    #[test]
    fn destroying_deletes_are_not_queued() {
        let f = fx(true);
        assert_eq!(
            queue_move(&f.db, f.acc, f.junk, &[3], MoveTarget::Trash).unwrap(),
            Queued::Permanent
        );
        let f = fx(false);
        assert_eq!(
            queue_move(&f.db, f.acc, f.inbox, &[1], MoveTarget::Trash).unwrap(),
            Queued::Permanent
        );
        assert_eq!(visible(&f, f.inbox), 2);
        let _ = f.trash;
    }

    #[test]
    fn move_and_archive_resolve_their_targets() {
        let f = fx(true);
        assert_eq!(
            queue_move(
                &f.db,
                f.acc,
                f.work,
                &[1],
                MoveTarget::Folder("Work".into())
            )
            .unwrap(),
            Queued::AlreadyThere
        );
        match queue_move(
            &f.db,
            f.acc,
            f.inbox,
            &[1],
            MoveTarget::Folder("Work".into()),
        )
        .unwrap()
        {
            Queued::Pending { label, .. } => assert_eq!(label, "Moved 1 to Work"),
            other => panic!("{other:?}"),
        }
        match queue_move(&f.db, f.acc, f.inbox, &[2], MoveTarget::Archive).unwrap() {
            Queued::Pending { label, .. } => assert_eq!(label, "Archived 1 to Archive"),
            other => panic!("{other:?}"),
        }
        assert!(queue_move(
            &f.db,
            f.acc,
            f.inbox,
            &[1],
            MoveTarget::Folder("Nope".into())
        )
        .is_err());
        assert!(queue_move(&f.db, f.acc, f.inbox, &[], MoveTarget::Trash).is_err());
        // A folder id from another account is refused, never acted on.
        assert!(queue_move(&f.db, f.acc + 1, f.inbox, &[1], MoveTarget::Trash).is_err());
    }
}
