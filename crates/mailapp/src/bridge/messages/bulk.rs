//! The multi-select actions behind the bulk action bar.
//!
//! Each one takes the selection as a JSON UID array from QML (see
//! `parse_uids_json`) and applies one statement or one IMAP command to the
//! whole set, rather than looping the single-message path -- a hundred
//! selected mails must not be a hundred round trips.

use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::store::{folders, messages};
use mailcore::undo::{self, MoveTarget, Queued};

use crate::bridge::qobject;
use crate::bridge::worker::{spawn_flag_push, spawn_job, spawn_push_after_grace, JobRefresh};
use crate::bridge::{push_feeds, qstring, shared_db};
use mailcore::sync::pool::{checkout_session, job_account};

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
                if n == 0 {
                    qstring("No messages changed")
                } else {
                    spawn_flag_push(acc_id);
                    if n == 1 {
                        qstring(if read {
                            "Marked 1 as read"
                        } else {
                            "Marked 1 as unread"
                        })
                    } else if read {
                        qstring(&format!("Marked {n} as read"))
                    } else {
                        qstring(&format!("Marked {n} as unread"))
                    }
                }
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
                if n == 0 {
                    qstring("No messages changed")
                } else {
                    spawn_flag_push(acc_id);
                    if n == 1 {
                        qstring(if starred { "Starred 1" } else { "Unstarred 1" })
                    } else if starred {
                        qstring(&format!("Starred {n}"))
                    } else {
                        qstring(&format!("Unstarred {n}"))
                    }
                }
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
        let groups = match parse_hits_json(&hits_json.to_string()) {
            Ok(g) => g,
            Err(e) => return qstring(&e),
        };
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let acc_id = *self.current_account_id();
        let mut resolved = Vec::with_capacity(groups.len());
        for (path, uids) in groups {
            match folders::get_by_path(db, acc_id, &path) {
                Ok(f) => resolved.push((f.id, uids)),
                Err(e) => return qstring(&e.to_string()),
            }
        }
        self.purge_groups(resolved)
    }

    /// Destroy `uids` of the current folder server-side (a job; no undo).
    pub(crate) fn purge_uids(self: Pin<&mut Self>, uids: Vec<u32>) -> QString {
        let folder_id = *self.current_folder_id();
        self.purge_groups(vec![(folder_id, uids)])
    }

    /// Destroy per-folder UID lists of the current account in one job.
    fn purge_groups(self: Pin<&mut Self>, groups: Vec<(i64, Vec<u32>)>) -> QString {
        let acc_id = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Delete", move |db, _progress| async move {
            let acc = job_account(db, acc_id)?;
            let mut imap = checkout_session(&acc).await?;
            // A failing folder must not hide what the others already
            // destroyed: report both and still refresh the feeds.
            let mut n = 0;
            let mut failed = None;
            for (group_folder, uids) in &groups {
                match imap.purge_uids(db, *group_folder, uids).await {
                    Ok(k) => n += k,
                    Err(e) => {
                        failed = Some(e.to_string());
                        break;
                    }
                }
            }
            let status = match failed {
                None => {
                    imap.checkin();
                    format!("Deleted {n} permanently")
                }
                Some(e) if n == 0 => return Err(e),
                Some(e) => format!("Deleted {n} permanently, then failed: {e}"),
            };
            Ok((status, Some(JobRefresh::feeds(acc.id, folder_id))))
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
