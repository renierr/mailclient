use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::feed;
use mailcore::store::{accounts, folders, messages, settings};
use mailcore::sync::imap::{ArchiveOutcome, MoveOutcome, TrashOutcome};

use crate::bridge::qobject;
use crate::bridge::session::{checkout_session, current_account};
use crate::bridge::worker::{spawn_flag_push, spawn_job, JobRefresh};
use crate::bridge::{open_db, push_feeds, qstring, MAX_MESSAGE_LIMIT};

mod attachments;
mod bulk;
mod files;

pub(crate) use attachments::{draft_attachment_path, ensure_attachment_data};
pub(crate) use files::{
    dir_to_path, file_url, numbered_filename, resolve_save_path, safe_filename,
};

/// Parse a bulk UID argument (JSON array of numbers from QML) into a
/// deduplicated, sorted UID list. Caps at `MAX_MESSAGE_LIMIT` so one click
/// cannot build an unbounded IMAP sequence set.
pub(crate) fn parse_uids_json(raw: &str) -> Result<Vec<u32>, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| "invalid selection".to_string())?;
    let arr = v
        .as_array()
        .ok_or_else(|| "invalid selection".to_string())?;
    if arr.is_empty() {
        return Err("no messages selected".to_string());
    }
    if arr.len() > MAX_MESSAGE_LIMIT as usize {
        return Err(format!(
            "too many messages selected (max {})",
            MAX_MESSAGE_LIMIT
        ));
    }
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        let uid = x.as_u64().ok_or_else(|| "invalid selection".to_string())? as u32;
        if uid == 0 {
            return Err("invalid selection".to_string());
        }
        out.push(uid);
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err("no messages selected".to_string());
    }
    Ok(out)
}

impl qobject::Bridge {
    pub fn message_html(&self, uid: i32, allow_remote: bool) -> QString {
        let Ok(db) = open_db() else {
            return qstring("");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("");
        }
        feed::message_html(&db, folder_id, uid as u32, allow_remote)
            .map_or_else(|_| qstring(""), |h| qstring(&h))
    }

    pub fn message_json(&self, uid: i32) -> QString {
        let Ok(db) = open_db() else {
            return qstring("{}");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("{}");
        }
        feed::message_json(&db, folder_id, uid as u32)
            .map_or_else(|_| qstring("{}"), |json| qstring(&json))
    }

    pub fn attachments_json(&self, uid: i32) -> QString {
        let Ok(db) = open_db() else {
            return qstring("[]");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("[]");
        }
        feed::attachments_json(&db, folder_id, uid as u32)
            .map_or_else(|_| qstring("[]"), |j| qstring(&j))
    }

    pub fn message_headers_json(&self, uid: i32) -> QString {
        let Ok(db) = open_db() else {
            return qstring("{}");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("{}");
        }
        feed::headers_json(&db, folder_id, uid as u32)
            .map_or_else(|_| qstring("{}"), |j| qstring(&j))
    }

    /// FTS search for the toolbar (up to 50 hits, rank order). `folder`
    /// scopes to one folder path (empty = whole account). Local SQLite
    /// read, no network — safe to call per keystroke.
    pub fn search_json(&self, query: &QString, folder: &QString) -> QString {
        let Ok(db) = open_db() else {
            return qstring("[]");
        };
        let acc_id = *self.current_account_id();
        if acc_id < 0 {
            return qstring("[]");
        }
        feed::search_json(&db, acc_id, &query.to_string(), 50, &folder.to_string())
            .map_or_else(|_| qstring("[]"), |j| qstring(&j))
    }

    pub fn open_attachment(self: Pin<&mut Self>, attachment_id: i32) -> QString {
        if attachment_id < 0 {
            return qstring("unknown attachment");
        }
        spawn_job(self, "Open", move |db, _progress| async move {
            let parent =
                messages::get_attachment(&db, attachment_id as i64).map_err(|e| e.to_string())?;
            ensure_attachment_data(&db, parent.message_id, false).await?;
            let a =
                messages::get_attachment(&db, attachment_id as i64).map_err(|e| e.to_string())?;
            let dir = std::env::temp_dir().join("mailclient-attachments");
            std::fs::create_dir_all(&dir).map_err(|e| format!("cannot use temp folder: {e}"))?;
            let name = format!(
                "{}-{}-{}",
                parent.message_id,
                attachment_id,
                safe_filename(a.filename.as_deref(), attachment_id as i64)
            );
            let dest = dir.join(name);
            messages::save_attachment_to_path(&db, attachment_id as i64, &dest)
                .map_err(|e| e.to_string())?;
            // Reading bytes out of a message changes nothing the feeds
            // show, so the list keeps its scroll position and selection.
            Ok((file_url(&dest), None))
        })
    }

    pub fn save_attachment(self: Pin<&mut Self>, attachment_id: i32, path: &QString) -> QString {
        if attachment_id < 0 {
            return qstring("unknown attachment");
        }
        let path = path.to_string();
        spawn_job(self, "Save", move |db, _progress| async move {
            let parent =
                messages::get_attachment(&db, attachment_id as i64).map_err(|e| e.to_string())?;
            ensure_attachment_data(&db, parent.message_id, false).await?;
            let dest = resolve_save_path(&db, attachment_id as i64, &path)?;
            messages::save_attachment_to_path(&db, attachment_id as i64, &dest)
                .map_err(|e| e.to_string())?;
            Ok((format!("Saved to {}", dest.display()), None))
        })
    }

    pub fn save_all_attachments(self: Pin<&mut Self>, uid: i32, dir: &QString) -> QString {
        let folder_id = *self.current_folder_id();
        let dir = dir.to_string();
        spawn_job(self, "Save", move |db, _progress| async move {
            if folder_id < 0 || uid < 0 {
                return Err("no message selected".to_string());
            }
            let msg = messages::get_by_uid(&db, folder_id, uid as u32)
                .map_err(|_| "unknown message".to_string())?;
            ensure_attachment_data(&db, msg.id, false).await?;
            let files = messages::list_attachments(&db, msg.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|a| !a.is_inline)
                .collect::<Vec<_>>();
            if files.is_empty() {
                return Err("no attachments to save".to_string());
            }
            let mut base = dir_to_path(&dir);
            std::fs::create_dir_all(&base).map_err(|e| format!("cannot create folder: {e}"))?;
            let mut saved = 0u32;
            for a in &files {
                let name = safe_filename(a.filename.as_deref(), a.id);
                base.push(&name);
                let mut n = 1;
                while base.exists() {
                    base.pop();
                    base.push(numbered_filename(&name, n));
                    n += 1;
                }
                match messages::save_attachment_to_path(&db, a.id, &base) {
                    Ok(_) => saved += 1,
                    Err(e) => {
                        log::warn!("save-all: {} failed: {e}", a.id);
                        base.pop();
                        continue;
                    }
                }
                base.pop();
            }
            if saved == 0 {
                return Err("could not save attachments".to_string());
            }
            Ok((format!("Saved {saved} attachment(s)"), None))
        })
    }

    pub fn open_message(mut self: Pin<&mut Self>, uid: i32) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let Ok(msg) = messages::get_by_uid(&db, folder_id, uid as u32) else {
            return qstring("");
        };
        if !msg.is_read {
            let _ = messages::set_flags(&db, msg.id, true, msg.is_starred);
            // Seen is pushed promptly in the background (plus again on the
            // next sync), so closing the app right after reading loses nothing.
            spawn_flag_push(acc_id);
        }
        // Refresh the QML-bound feeds so the follow-up reloadMessages() /
        // reloadFolders() in QML see the cleared unread flag immediately.
        // Without this messages_json/folders_json stay stale and the marker
        // only clears on the next folder switch (which pushes feeds).
        push_feeds(&mut self, &db, acc_id, folder_id);
        qstring("")
    }

    pub fn mark_read(mut self: Pin<&mut Self>, uid: i32, read: bool) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let Ok(msg) = messages::get_by_uid(&db, folder_id, uid as u32) else {
            return qstring("");
        };
        // Queued locally, pushed promptly in the background (see open_message).
        let _ = messages::set_flags(&db, msg.id, read, msg.is_starred);
        spawn_flag_push(acc_id);
        push_feeds(&mut self, &db, acc_id, folder_id);
        qstring("")
    }

    pub fn toggle_star(mut self: Pin<&mut Self>, uid: i32) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let Ok(msg) = messages::get_by_uid(&db, folder_id, uid as u32) else {
            return qstring("");
        };
        // Queued locally, pushed promptly in the background (see open_message).
        let _ = messages::set_flags(&db, msg.id, msg.is_read, !msg.is_starred);
        spawn_flag_push(acc_id);
        push_feeds(&mut self, &db, acc_id, folder_id);
        qstring("")
    }

    pub fn delete_message(self: Pin<&mut Self>, uid: i32) -> QString {
        let acc_id = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Delete", move |db, _progress| async move {
            let msg = messages::get_by_uid(&db, folder_id, uid as u32)
                .map_err(|_| "message is no longer available".to_string())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let mut imap = checkout_session(&acc).await?;
            let outcome = imap
                .trash_message(&db, msg.id)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                match outcome {
                    TrashOutcome::Moved(path) => format!("Moved to {path}"),
                    TrashOutcome::Expunged => "Deleted permanently".to_string(),
                },
                Some(JobRefresh::feeds(acc_id, folder_id)),
            ))
        })
    }

    pub fn archive_message(self: Pin<&mut Self>, uid: i32) -> QString {
        let acc_id = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Archive", move |db, _progress| async move {
            let msg = messages::get_by_uid(&db, folder_id, uid as u32)
                .map_err(|_| "message is no longer available".to_string())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let mut imap = checkout_session(&acc).await?;
            let outcome = imap
                .archive_message(&db, msg.id)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                match outcome {
                    ArchiveOutcome::Moved(path) => format!("Archived to {path}"),
                    ArchiveOutcome::AlreadyThere => "Already in Archive".to_string(),
                },
                Some(JobRefresh::feeds(acc_id, folder_id)),
            ))
        })
    }

    pub fn move_message(self: Pin<&mut Self>, uid: i32, path: &QString) -> QString {
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let path = path.to_string();
        spawn_job(self, "Move", move |db, _progress| async move {
            let acc = current_account(&db, wanted)?;
            let msg = messages::get_by_uid(&db, current, uid as u32)
                .map_err(|_| "message is no longer available".to_string())?;
            let dest = folders::get_by_path(&db, acc.id, &path).map_err(|e| e.to_string())?;
            let mut imap = checkout_session(&acc).await?;
            let outcome = imap
                .move_to_folder(&db, msg.id, dest.id)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                match outcome {
                    MoveOutcome::Moved(path) => format!("Moved to {path}"),
                    MoveOutcome::AlreadyThere => "Already here".to_string(),
                },
                Some(JobRefresh::feeds(acc.id, current)),
            ))
        })
    }

    pub fn purge_message(self: Pin<&mut Self>, uid: i32) -> QString {
        let acc_id = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Delete", move |db, _progress| async move {
            let msg = messages::get_by_uid(&db, folder_id, uid as u32)
                .map_err(|_| "message is no longer available".to_string())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let mut imap = checkout_session(&acc).await?;
            imap.delete_message(&db, msg.id)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                "Deleted permanently".to_string(),
                Some(JobRefresh::feeds(acc_id, folder_id)),
            ))
        })
    }

    pub fn set_sort(mut self: Pin<&mut Self>, field: &QString, descending: bool) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let normalized = settings::normalize_sort_field(&field.to_string()).to_string();
        if let Err(e) = settings::set_sort(&db, &normalized, descending) {
            return qstring(&e.to_string());
        }
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        // push_feeds re-reads the settings for the feed and syncs the props.
        push_feeds(&mut self, &db, acc_id, folder_id);
        qstring("")
    }
}
