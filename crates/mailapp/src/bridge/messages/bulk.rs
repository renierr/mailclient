//! The multi-select actions behind the bulk action bar.
//!
//! Each one takes the selection from QML as a JSON UID array of one folder
//! (`folder_id`, see `parse_uids_json`) or as search hits of one account
//! (`account_id`, see `parse_hits_json`) and applies one statement or one
//! IMAP command to the whole set, rather than looping the single-message path -- a hundred
//! selected mails must not be a hundred round trips.

use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::bulk;
use mailcore::store::messages;
use mailcore::undo::{self, MoveTarget, Queued};

use crate::bridge::qobject;
use crate::bridge::worker::{spawn_flag_push, spawn_job, spawn_push_after_grace, JobRefresh};
use crate::bridge::{folder_account, push_feeds, push_flag_change, qstring, shared_db};
use mailcore::sync::pool::job_account;

use super::{parse_hits_json, parse_uids_json};

impl qobject::Bridge {
    pub fn mark_read_many(
        mut self: Pin<&mut Self>,
        folder_id: i64,
        uids_json: &QString,
        read: bool,
    ) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let acc_id = match folder_account(db, folder_id) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        match messages::set_read_many_by_uids(db, folder_id, &uids, read) {
            Ok(n) => {
                // QML patches the selected rows (`message_rows_json`).
                push_flag_change(&mut self, db);
                if n > 0 {
                    spawn_flag_push(acc_id);
                }
                qstring(&read_label(n, read))
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn set_star_many(
        mut self: Pin<&mut Self>,
        folder_id: i64,
        uids_json: &QString,
        starred: bool,
    ) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let acc_id = match folder_account(db, folder_id) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        match messages::set_star_many_by_uids(db, folder_id, &uids, starred) {
            Ok(n) => {
                // QML patches the selected rows (`message_rows_json`).
                push_flag_change(&mut self, db);
                if n > 0 {
                    spawn_flag_push(acc_id);
                }
                qstring(&star_label(n, starred))
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn delete_many(self: Pin<&mut Self>, folder_id: i64, uids_json: &QString) -> QString {
        match parse_uids_json(&uids_json.to_string()) {
            Ok(uids) => self.queue_undoable(folder_id, uids, MoveTarget::Trash),
            Err(e) => qstring(&e),
        }
    }

    pub fn archive_many(self: Pin<&mut Self>, folder_id: i64, uids_json: &QString) -> QString {
        match parse_uids_json(&uids_json.to_string()) {
            Ok(uids) => self.queue_undoable(folder_id, uids, MoveTarget::Archive),
            Err(e) => qstring(&e),
        }
    }

    pub fn move_many(
        self: Pin<&mut Self>,
        folder_id: i64,
        uids_json: &QString,
        path: &QString,
    ) -> QString {
        match parse_uids_json(&uids_json.to_string()) {
            Ok(uids) => self.queue_undoable(folder_id, uids, MoveTarget::Folder(path.to_string())),
            Err(e) => qstring(&e),
        }
    }

    pub fn purge_many(self: Pin<&mut Self>, folder_id: i64, uids_json: &QString) -> QString {
        match parse_uids_json(&uids_json.to_string()) {
            Ok(uids) => self.purge_uids(folder_id, uids),
            Err(e) => qstring(&e),
        }
    }

    /// Permanently destroy search hits across folders: one job, one IMAP
    /// session, no folder switch. Per-folder `purge_many` calls would each
    /// be a job, and every one after the first is refused while the bridge
    /// is busy.
    pub fn purge_hits(self: Pin<&mut Self>, account_id: i64, hits_json: &QString) -> QString {
        match hit_groups(account_id, hits_json) {
            Ok(groups) => self.purge_groups(account_id, groups),
            Err(e) => qstring(&e),
        }
    }

    pub fn mark_read_hits(
        mut self: Pin<&mut Self>,
        account_id: i64,
        hits_json: &QString,
        read: bool,
    ) -> QString {
        let groups = match hit_groups(account_id, hits_json) {
            Ok(g) => g,
            Err(e) => return qstring(&e),
        };
        self.as_mut().flag_hits(
            account_id,
            |db| bulk::set_read(db, &groups, read),
            |n| read_label(n, read),
        )
    }

    pub fn set_star_hits(
        mut self: Pin<&mut Self>,
        account_id: i64,
        hits_json: &QString,
        starred: bool,
    ) -> QString {
        let groups = match hit_groups(account_id, hits_json) {
            Ok(g) => g,
            Err(e) => return qstring(&e),
        };
        self.as_mut().flag_hits(
            account_id,
            |db| bulk::set_starred(db, &groups, starred),
            |n| star_label(n, starred),
        )
    }

    pub fn delete_hits(self: Pin<&mut Self>, account_id: i64, hits_json: &QString) -> QString {
        self.queue_hits(account_id, hits_json, MoveTarget::Trash)
    }

    pub fn archive_hits(self: Pin<&mut Self>, account_id: i64, hits_json: &QString) -> QString {
        self.queue_hits(account_id, hits_json, MoveTarget::Archive)
    }

    pub fn move_hits(
        self: Pin<&mut Self>,
        account_id: i64,
        hits_json: &QString,
        path: &QString,
    ) -> QString {
        self.queue_hits(account_id, hits_json, MoveTarget::Folder(path.to_string()))
    }

    /// A local flag write over hits of `account_id`, then the feeds and the
    /// flag push.
    fn flag_hits(
        mut self: Pin<&mut Self>,
        account_id: i64,
        write: impl FnOnce(&mailcore::Db) -> mailcore::Result<u64>,
        label: impl FnOnce(u64) -> String,
    ) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        match write(db) {
            Ok(n) => {
                let (shown_account, shown_folder) = self.shown();
                push_feeds(&mut self, db, shown_account, shown_folder);
                if n > 0 {
                    spawn_flag_push(account_id);
                }
                qstring(&label(n))
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    /// Delete / archive / move over hits: one Undo for what goes through the
    /// undo queue, one purge job for the shares that destroy.
    fn queue_hits(
        mut self: Pin<&mut Self>,
        account_id: i64,
        hits_json: &QString,
        target: MoveTarget,
    ) -> QString {
        let groups = match hit_groups(account_id, hits_json) {
            Ok(g) => g,
            Err(e) => return qstring(&e),
        };
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let moved = match bulk::queue_move(db, account_id, &groups, target) {
            Ok(m) => m,
            Err(e) => return qstring(&e),
        };
        let (shown_account, shown_folder) = self.shown();
        push_feeds(&mut self, db, shown_account, shown_folder);
        if let Some(batch) = &moved.batch {
            self.as_mut()
                .undo_available(&qstring(batch), &qstring(&moved.label));
            spawn_push_after_grace(account_id);
        }
        if moved.permanent.is_empty() {
            return qstring(&moved.label);
        }
        let purging = self.purge_groups(account_id, moved.permanent);
        if moved.batch.is_some() && purging.is_empty() {
            qstring(&moved.label)
        } else {
            purging
        }
    }

    /// Destroy `uids` of `folder_id` server-side (a job; no undo).
    pub(crate) fn purge_uids(self: Pin<&mut Self>, folder_id: i64, uids: Vec<u32>) -> QString {
        let account_id = match shared_db().and_then(|db| folder_account(db, folder_id)) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        self.purge_groups(account_id, vec![(folder_id, uids)])
    }

    /// Destroy per-folder UID lists of `account_id` in one job.
    fn purge_groups(self: Pin<&mut Self>, account_id: i64, groups: bulk::Groups) -> QString {
        let (shown_account, shown_folder) = self.shown();
        spawn_job(self, "Delete", move |db, _progress| async move {
            let acc = job_account(db, account_id)?;
            let purged = bulk::purge(db, &acc, &groups).await?;
            Ok((
                purged.status(),
                Some(JobRefresh::feeds(shown_account, shown_folder)),
            ))
        })
    }

    /// Delete / archive / move through the undo queue (`mailcore::undo`):
    /// hide now, announce the Undo, push after the grace period. A delete
    /// that can only destroy falls through to [`Self::purge_uids`] — QML has
    /// already confirmed it as permanent.
    pub(crate) fn queue_undoable(
        mut self: Pin<&mut Self>,
        folder_id: i64,
        uids: Vec<u32>,
        target: MoveTarget,
    ) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let acc_id = match folder_account(db, folder_id) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        match undo::queue_move(db, acc_id, folder_id, &uids, target) {
            Ok(Queued::Pending { batch, label, .. }) => {
                let (shown_account, shown_folder) = self.shown();
                push_feeds(&mut self, db, shown_account, shown_folder);
                self.as_mut()
                    .undo_available(&qstring(&batch), &qstring(&label));
                spawn_push_after_grace(acc_id);
                qstring(&label)
            }
            Ok(Queued::Permanent) => self.purge_uids(folder_id, uids),
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
            Ok(n) => {
                if n > 0 {
                    let (acc_id, folder_id) = self.shown();
                    push_feeds(&mut self, db, acc_id, folder_id);
                }
                qstring(&undo::undone_text(n))
            }
            Err(e) => qstring(&e),
        }
    }

    pub fn undo_grace_secs(&self) -> i32 {
        undo::UNDO_GRACE_SECS as i32
    }
}

/// Search hits of `account_id`, grouped by folder by the core.
fn hit_groups(account_id: i64, hits_json: &QString) -> Result<bulk::Groups, String> {
    let hits = parse_hits_json(&hits_json.to_string())?;
    let db = shared_db()?;
    bulk::resolve_hits(db, account_id, &hits)
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
