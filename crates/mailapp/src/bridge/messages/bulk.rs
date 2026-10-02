//! The multi-select actions behind the bulk action bar.
//!
//! Each one takes the selection as a JSON UID array from QML (see
//! `parse_uids_json`) and applies one statement or one IMAP command to the
//! whole set, rather than looping the single-message path -- a hundred
//! selected mails must not be a hundred round trips.

use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::bulk;
use mailcore::store::messages;
use mailcore::undo::{self, MoveTarget, Queued};

use crate::bridge::qobject;
use crate::bridge::worker::{spawn_flag_push, spawn_job, spawn_push_after_grace, JobRefresh};
use crate::bridge::{push_feeds, qstring, shared_db};
use mailcore::sync::pool::job_account;

use super::{parse_hits_json, parse_uids_json};

impl qobject::Bridge {
    pub fn mark_read_many(mut self: Pin<&mut Self>, uids_json: &QString, read: bool) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        if folder_id < 0 {
            return qstring("no folder selected");
        }
        match messages::set_read_many_by_uids(db, folder_id, &uids, read) {
            Ok(n) => {
                push_feeds(&mut self, db, acc_id, folder_id);
                if n > 0 {
                    spawn_flag_push(acc_id);
                }
                qstring(&read_label(n, read))
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn set_star_many(mut self: Pin<&mut Self>, uids_json: &QString, starred: bool) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        if folder_id < 0 {
            return qstring("no folder selected");
        }
        match messages::set_star_many_by_uids(db, folder_id, &uids, starred) {
            Ok(n) => {
                push_feeds(&mut self, db, acc_id, folder_id);
                if n > 0 {
                    spawn_flag_push(acc_id);
                }
                qstring(&star_label(n, starred))
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn delete_many(self: Pin<&mut Self>, uids_json: &QString) -> QString {
        match parse_uids_json(&uids_json.to_string()) {
            Ok(uids) => self.queue_undoable(uids, MoveTarget::Trash),
            Err(e) => qstring(&e),
        }
    }

    pub fn archive_many(self: Pin<&mut Self>, uids_json: &QString) -> QString {
        match parse_uids_json(&uids_json.to_string()) {
            Ok(uids) => self.queue_undoable(uids, MoveTarget::Archive),
            Err(e) => qstring(&e),
        }
    }

    pub fn move_many(self: Pin<&mut Self>, uids_json: &QString, path: &QString) -> QString {
        match parse_uids_json(&uids_json.to_string()) {
            Ok(uids) => self.queue_undoable(uids, MoveTarget::Folder(path.to_string())),
            Err(e) => qstring(&e),
        }
    }

    pub fn purge_many(self: Pin<&mut Self>, uids_json: &QString) -> QString {
        match parse_uids_json(&uids_json.to_string()) {
            Ok(uids) => self.purge_uids(uids),
            Err(e) => qstring(&e),
        }
    }

    /// Permanently destroy search hits across folders: one job, one IMAP
    /// session, no folder switch. Per-folder `purge_many` calls would each
    /// be a job, and every one after the first is refused while the bridge
    /// is busy.
    pub fn purge_hits(self: Pin<&mut Self>, hits_json: &QString) -> QString {
        match self.hit_groups(hits_json) {
            Ok(groups) => self.purge_groups(groups),
            Err(e) => qstring(&e),
        }
    }

    pub fn mark_read_hits(mut self: Pin<&mut Self>, hits_json: &QString, read: bool) -> QString {
        let groups = match self.hit_groups(hits_json) {
            Ok(g) => g,
            Err(e) => return qstring(&e),
        };
        self.as_mut().flag_hits(
            |db| bulk::set_read(db, &groups, read),
            |n| read_label(n, read),
        )
    }

    pub fn set_star_hits(mut self: Pin<&mut Self>, hits_json: &QString, starred: bool) -> QString {
        let groups = match self.hit_groups(hits_json) {
            Ok(g) => g,
            Err(e) => return qstring(&e),
        };
        self.as_mut().flag_hits(
            |db| bulk::set_starred(db, &groups, starred),
            |n| star_label(n, starred),
        )
    }

    pub fn delete_hits(self: Pin<&mut Self>, hits_json: &QString) -> QString {
        self.queue_hits(hits_json, MoveTarget::Trash)
    }

    pub fn archive_hits(self: Pin<&mut Self>, hits_json: &QString) -> QString {
        self.queue_hits(hits_json, MoveTarget::Archive)
    }

    pub fn move_hits(self: Pin<&mut Self>, hits_json: &QString, path: &QString) -> QString {
        self.queue_hits(hits_json, MoveTarget::Folder(path.to_string()))
    }

    /// Search hits of the current account, grouped by folder by the core.
    fn hit_groups(&self, hits_json: &QString) -> Result<bulk::Groups, String> {
        let hits = parse_hits_json(&hits_json.to_string())?;
        let db = shared_db()?;
        bulk::resolve_hits(db, *self.current_account_id(), &hits)
    }

    /// A local flag write over hits, then the feeds and the flag push.
    fn flag_hits(
        mut self: Pin<&mut Self>,
        write: impl FnOnce(&mailcore::Db) -> mailcore::Result<u64>,
        label: impl FnOnce(u64) -> String,
    ) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        match write(db) {
            Ok(n) => {
                push_feeds(&mut self, db, acc_id, folder_id);
                if n > 0 {
                    spawn_flag_push(acc_id);
                }
                qstring(&label(n))
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    /// Delete / archive / move over hits: one Undo for what goes through the
    /// undo queue, one purge job for the shares that destroy.
    fn queue_hits(mut self: Pin<&mut Self>, hits_json: &QString, target: MoveTarget) -> QString {
        let groups = match self.hit_groups(hits_json) {
            Ok(g) => g,
            Err(e) => return qstring(&e),
        };
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let moved = match bulk::queue_move(db, acc_id, &groups, target) {
            Ok(m) => m,
            Err(e) => return qstring(&e),
        };
        push_feeds(&mut self, db, acc_id, folder_id);
        if let Some(batch) = &moved.batch {
            self.as_mut()
                .undo_available(&qstring(batch), &qstring(&moved.label));
            spawn_push_after_grace(acc_id);
        }
        if moved.permanent.is_empty() {
            return qstring(&moved.label);
        }
        let purging = self.purge_groups(moved.permanent);
        if moved.batch.is_some() && purging.is_empty() {
            qstring(&moved.label)
        } else {
            purging
        }
    }

    /// Destroy `uids` of the current folder server-side (a job; no undo).
    pub(crate) fn purge_uids(self: Pin<&mut Self>, uids: Vec<u32>) -> QString {
        let folder_id = *self.current_folder_id();
        self.purge_groups(vec![(folder_id, uids)])
    }

    /// Destroy per-folder UID lists of the current account in one job.
    fn purge_groups(self: Pin<&mut Self>, groups: bulk::Groups) -> QString {
        let acc_id = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Delete", move |db, _progress| async move {
            let acc = job_account(db, acc_id)?;
            let purged = bulk::purge(db, &acc, &groups).await?;
            Ok((purged.status(), Some(JobRefresh::feeds(acc.id, folder_id))))
        })
    }

    /// Delete / archive / move through the undo queue (`mailcore::undo`):
    /// hide now, announce the Undo, push after the grace period. A delete
    /// that can only destroy falls through to [`Self::purge_uids`] — QML has
    /// already confirmed it as permanent.
    pub(crate) fn queue_undoable(
        mut self: Pin<&mut Self>,
        uids: Vec<u32>,
        target: MoveTarget,
    ) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        if folder_id < 0 {
            return qstring("no folder selected");
        }
        match undo::queue_move(db, acc_id, folder_id, &uids, target) {
            Ok(Queued::Pending { batch, label, .. }) => {
                push_feeds(&mut self, db, acc_id, folder_id);
                self.as_mut()
                    .undo_available(&qstring(&batch), &qstring(&label));
                spawn_push_after_grace(acc_id);
                qstring(&label)
            }
            Ok(Queued::Permanent) => self.purge_uids(uids),
            Ok(Queued::AlreadyThere) => qstring("Already here"),
            Err(e) => qstring(&e),
        }
    }

    pub fn undo_move(mut self: Pin<&mut Self>, batch: &QString) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        match undo::undo(db, &batch.to_string()) {
            Ok(0) => qstring("Too late to undo — already done on the server"),
            Ok(n) => {
                let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
                push_feeds(&mut self, db, acc_id, folder_id);
                qstring(&if n == 1 {
                    "Undone: 1 message is back".to_string()
                } else {
                    format!("Undone: {n} messages are back")
                })
            }
            Err(e) => qstring(&e),
        }
    }

    pub fn undo_grace_secs(&self) -> i32 {
        undo::UNDO_GRACE_SECS as i32
    }
}

/// The status line after a read/unread change of `n` messages.
fn read_label(n: u64, read: bool) -> String {
    match (n, read) {
        (0, _) => "No messages changed".to_string(),
        (n, true) => format!("Marked {n} as read"),
        (n, false) => format!("Marked {n} as unread"),
    }
}

/// The status line after a star change of `n` messages.
fn star_label(n: u64, starred: bool) -> String {
    match (n, starred) {
        (0, _) => "No messages changed".to_string(),
        (n, true) => format!("Starred {n}"),
        (n, false) => format!("Unstarred {n}"),
    }
}
