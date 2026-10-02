//! Actions on a selection that may span folders: the checkbox set over
//! search hits. Both frontends hand the hits over as `(folder path, uid)`
//! and get one result back — one flag write, one Undo for a move that
//! touched several folders, one purge job — instead of looping folder by
//! folder and stitching the results together themselves.

use crate::models::Account;
use crate::store::{folders, messages};
use crate::sync::pool::checkout_session;
use crate::undo::{self, MoveTarget, Share};
use crate::Db;

/// A selection by folder: `(folder_id, uids)`, folders in the order their
/// first hit came.
pub type Groups = Vec<(i64, Vec<u32>)>;

/// Group `hits` (`(folder path, uid)`) of one account by folder. An unknown
/// folder fails the whole selection rather than acting on part of it.
pub fn resolve_hits(db: &Db, account_id: i64, hits: &[(String, u32)]) -> Result<Groups, String> {
    let mut by_path: Vec<(&str, Vec<u32>)> = Vec::new();
    for (path, uid) in hits {
        match by_path.iter_mut().find(|(p, _)| p == path) {
            Some((_, uids)) => {
                if !uids.contains(uid) {
                    uids.push(*uid);
                }
            }
            None => by_path.push((path, vec![*uid])),
        }
    }
    by_path
        .into_iter()
        .map(|(path, uids)| {
            folders::get_by_path(db, account_id, path)
                .map(|f| (f.id, uids))
                .map_err(|_| format!("unknown folder {path}"))
        })
        .collect()
}

/// Mark every message of `groups` read or unread; how many changed.
pub fn set_read(db: &Db, groups: &Groups, read: bool) -> crate::Result<u64> {
    let mut n = 0;
    for (folder_id, uids) in groups {
        n += messages::set_read_many_by_uids(db, *folder_id, uids, read)?;
    }
    Ok(n)
}

/// Star or unstar every message of `groups`; how many changed.
pub fn set_starred(db: &Db, groups: &Groups, starred: bool) -> crate::Result<u64> {
    let mut n = 0;
    for (folder_id, uids) in groups {
        n += messages::set_star_many_by_uids(db, *folder_id, uids, starred)?;
    }
    Ok(n)
}

/// What a move across folders came to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Moved {
    /// The Undo handle for every folder's share, `None` when nothing was
    /// queued.
    pub batch: Option<String>,
    /// The Undo toast's text, or why nothing happened.
    pub label: String,
    /// Shares that cannot go to Trash and destroy instead: the caller
    /// purges them ([`purge`]) once the user has confirmed that.
    pub permanent: Groups,
}

/// Queue an undoable delete, archive or move for every folder of `groups`
/// under one Undo batch.
pub fn queue_move(
    db: &Db,
    account_id: i64,
    groups: &Groups,
    target: MoveTarget,
) -> Result<Moved, String> {
    let batch = uuid::Uuid::new_v4().to_string();
    let mut out = Moved::default();
    let mut queued: Option<(crate::store::pending_moves::PendingAction, String)> = None;
    let mut count = 0;
    for (folder_id, uids) in groups {
        match undo::queue_into(db, account_id, *folder_id, uids, &target, &batch)? {
            Share::Pending {
                count: n,
                action,
                place,
            } => {
                count += n;
                queued.get_or_insert((action, place));
            }
            Share::Permanent => out.permanent.push((*folder_id, uids.clone())),
            Share::AlreadyThere => {}
        }
    }
    match queued {
        Some((action, place)) => {
            out.label = undo::label(action, count, &place);
            out.batch = Some(batch);
        }
        None if out.permanent.is_empty() => out.label = "Already here".to_string(),
        None => {}
    }
    Ok(out)
}

/// What a purge destroyed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Purged {
    pub deleted: u64,
    /// The folder that failed, after the ones before it were destroyed.
    pub failed: Option<String>,
}

impl Purged {
    /// The status line: what was destroyed, and where it stopped.
    #[must_use]
    pub fn status(&self) -> String {
        match &self.failed {
            None => format!("Deleted {} permanently", self.deleted),
            Some(e) => format!("Deleted {} permanently, then failed: {e}", self.deleted),
        }
    }
}

/// Destroy every message of `groups` server-side over one pooled session
/// (`\Deleted` + expunge, no undo). A failing folder stops the run but does
/// not hide what the others already destroyed; nothing destroyed at all is
/// an error.
pub async fn purge(db: &Db, account: &Account, groups: &Groups) -> Result<Purged, String> {
    for (folder_id, _) in groups {
        let f = folders::get(db, *folder_id).map_err(|e| e.to_string())?;
        if f.account_id != account.id {
            return Err("folder does not belong to this account".to_string());
        }
    }
    let mut imap = checkout_session(account).await?;
    let mut deleted = 0;
    for (folder_id, uids) in groups {
        match imap.purge_uids(db, *folder_id, uids).await {
            Ok(n) => deleted += n,
            Err(e) if deleted == 0 => return Err(e.to_string()),
            Err(e) => {
                return Ok(Purged {
                    deleted,
                    failed: Some(e.to_string()),
                })
            }
        }
    }
    imap.checkin();
    Ok(Purged {
        deleted,
        failed: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::FolderRole;
    use crate::store::accounts;

    struct Fx {
        db: Db,
        acc: i64,
        inbox: i64,
        work: i64,
        junk: i64,
    }

    fn fx() -> Fx {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create_for_test(&db, "me@example.com");
        let inbox = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        let work = folders::upsert(&db, acc, "Work", "/", FolderRole::Custom).unwrap();
        let junk = folders::upsert(&db, acc, "Junk", "/", FolderRole::Junk).unwrap();
        folders::upsert(&db, acc, "Trash", "/", FolderRole::Trash).unwrap();
        for (folder, uid) in [(inbox, 1), (inbox, 2), (work, 1), (junk, 5)] {
            let mut m = messages::sample_new(acc, folder, uid);
            m.is_read = false;
            messages::upsert(&db, &m).unwrap();
        }
        Fx {
            db,
            acc,
            inbox,
            work,
            junk,
        }
    }

    fn hits(list: &[(&str, u32)]) -> Vec<(String, u32)> {
        list.iter().map(|(p, u)| ((*p).to_string(), *u)).collect()
    }

    #[test]
    fn hits_group_by_folder_in_order_of_their_first_hit() {
        let f = fx();
        let g = resolve_hits(
            &f.db,
            f.acc,
            &hits(&[("Work", 1), ("INBOX", 2), ("Work", 1), ("INBOX", 1)]),
        )
        .unwrap();
        assert_eq!(g, vec![(f.work, vec![1]), (f.inbox, vec![2, 1])]);
        assert!(resolve_hits(&f.db, f.acc, &hits(&[("Nope", 1)])).is_err());
    }

    #[test]
    fn flags_change_across_folders_in_one_call() {
        let f = fx();
        let g = vec![(f.inbox, vec![1, 2]), (f.work, vec![1])];
        assert_eq!(set_read(&f.db, &g, true).unwrap(), 3);
        assert_eq!(set_starred(&f.db, &g, true).unwrap(), 3);
        assert!(messages::get_by_uid(&f.db, f.work, 1).unwrap().is_read);
    }

    #[test]
    fn a_move_across_folders_is_one_undo() {
        let f = fx();
        let g = vec![(f.inbox, vec![1]), (f.work, vec![1])];
        let moved = queue_move(&f.db, f.acc, &g, MoveTarget::Trash).unwrap();
        assert_eq!(moved.label, "Moved 2 to Trash");
        assert!(moved.permanent.is_empty());
        let batch = moved.batch.unwrap();
        assert_eq!(undo::undo(&f.db, &batch).unwrap(), 2);
    }

    #[test]
    fn junk_shares_are_left_to_the_caller_to_purge() {
        let f = fx();
        let g = vec![(f.inbox, vec![1]), (f.junk, vec![5])];
        let moved = queue_move(&f.db, f.acc, &g, MoveTarget::Trash).unwrap();
        assert_eq!(moved.label, "Moved 1 to Trash");
        assert_eq!(moved.permanent, vec![(f.junk, vec![5])]);
        let only_junk =
            queue_move(&f.db, f.acc, &vec![(f.junk, vec![5])], MoveTarget::Trash).unwrap();
        assert_eq!(only_junk.batch, None);
        assert_eq!(only_junk.label, "");
    }

    #[test]
    fn purge_status_says_where_it_stopped() {
        let done = Purged {
            deleted: 3,
            failed: None,
        };
        assert_eq!(done.status(), "Deleted 3 permanently");
        let partial = Purged {
            deleted: 2,
            failed: Some("NO".into()),
        };
        assert_eq!(partial.status(), "Deleted 2 permanently, then failed: NO");
    }
}
