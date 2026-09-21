//! The multi-select actions behind the bulk action bar.
//!
//! Each one takes the selection as a JSON UID array from QML (see
//! `parse_uids_json`) and applies one statement or one IMAP command to the
//! whole set, rather than looping the single-message path -- a hundred
//! selected mails must not be a hundred round trips.

use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::models::FolderRole;
use mailcore::store::{accounts, folders, messages};

use crate::bridge::qobject;
use crate::bridge::session::{checkout_session, current_account};
use crate::bridge::worker::{spawn_flag_push, spawn_job, JobRefresh};
use crate::bridge::{open_db, push_feeds, qstring};

use super::parse_uids_json;

impl qobject::Bridge {
    pub fn mark_read_many(mut self: Pin<&mut Self>, uids_json: &QString, read: bool) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        if folder_id < 0 {
            return qstring("no folder selected");
        }
        match messages::set_read_many_by_uids(&db, folder_id, &uids, read) {
            Ok(n) => {
                push_feeds(&mut self, &db, acc_id, folder_id);
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
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        if folder_id < 0 {
            return qstring("no folder selected");
        }
        match messages::set_star_many_by_uids(&db, folder_id, &uids, starred) {
            Ok(n) => {
                push_feeds(&mut self, &db, acc_id, folder_id);
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
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let acc_id = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Delete", move |db, _progress| async move {
            let folder = folders::get(&db, folder_id).map_err(|e| e.to_string())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            if folder.account_id != acc.id {
                return Err("folder does not belong to this account".to_string());
            }
            let mut imap = checkout_session(&acc).await?;
            let summary = if folder.role == FolderRole::Junk {
                let n = imap
                    .purge_uids(&db, folder_id, &uids)
                    .await
                    .map_err(|e| e.to_string())?;
                format!("Deleted {n} permanently")
            } else {
                let trash = folders::list_by_account(&db, acc.id)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .find(|f| f.role == FolderRole::Trash);
                match trash.filter(|t| t.id != folder.id) {
                    Some(t) => {
                        let n = imap
                            .move_uids_to(&db, folder_id, &uids, &t.path)
                            .await
                            .map_err(|e| e.to_string())?;
                        format!("Moved {n} to {}", t.path)
                    }
                    None => {
                        let n = imap
                            .purge_uids(&db, folder_id, &uids)
                            .await
                            .map_err(|e| e.to_string())?;
                        format!("Deleted {n} permanently")
                    }
                }
            };
            imap.checkin();
            Ok((summary, Some(JobRefresh::feeds(acc_id, folder_id))))
        })
    }

    pub fn archive_many(self: Pin<&mut Self>, uids_json: &QString) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let acc_id = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Archive", move |db, _progress| async move {
            let folder = folders::get(&db, folder_id).map_err(|e| e.to_string())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let mut imap = checkout_session(&acc).await?;
            let archive = match folders::list_by_account(&db, acc.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|f| f.role == FolderRole::Archive)
            {
                Some(a) => a,
                None => {
                    let delim = folders::list_by_account(&db, acc.id)
                        .unwrap_or_default()
                        .first()
                        .map(|f| f.delimiter.clone())
                        .unwrap_or_else(|| "/".to_string());
                    imap.create_folder_path(&db, acc.id, "Archive", &delim)
                        .await
                        .map_err(|e| e.to_string())?
                }
            };
            let summary = if archive.id == folder.id {
                "Already in Archive".to_string()
            } else {
                let n = imap
                    .move_uids_to(&db, folder_id, &uids, &archive.path)
                    .await
                    .map_err(|e| e.to_string())?;
                format!("Archived {n} to {}", archive.path)
            };
            imap.checkin();
            Ok((summary, Some(JobRefresh::feeds(acc_id, folder_id))))
        })
    }

    pub fn move_many(self: Pin<&mut Self>, uids_json: &QString, path: &QString) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let path = path.to_string();
        spawn_job(self, "Move", move |db, _progress| async move {
            let acc = current_account(&db, wanted)?;
            let dest = folders::get_by_path(&db, acc.id, &path).map_err(|e| e.to_string())?;
            if dest.id == current {
                return Ok((
                    "Already here".to_string(),
                    Some(JobRefresh::feeds(acc.id, current)),
                ));
            }
            let folder = folders::get(&db, current).map_err(|e| e.to_string())?;
            if folder.account_id != acc.id {
                return Err("folder does not belong to this account".to_string());
            }
            let mut imap = checkout_session(&acc).await?;
            let n = imap
                .move_uids_to(&db, current, &uids, &dest.path)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                format!("Moved {n} to {}", dest.path),
                Some(JobRefresh::feeds(acc.id, current)),
            ))
        })
    }

    pub fn purge_many(self: Pin<&mut Self>, uids_json: &QString) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let acc_id = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Delete", move |db, _progress| async move {
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let mut imap = checkout_session(&acc).await?;
            let n = imap
                .purge_uids(&db, folder_id, &uids)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                format!("Deleted {n} permanently"),
                Some(JobRefresh::feeds(acc.id, folder_id)),
            ))
        })
    }
}
