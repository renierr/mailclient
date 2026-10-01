use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::feed;
use mailcore::store::{messages, settings};
use mailcore::undo::MoveTarget;

use crate::bridge::qobject;
use crate::bridge::worker::{spawn_flag_push, spawn_job};
use crate::bridge::{push_feeds, qstring, shared_db, MAX_MESSAGE_LIMIT};

mod attachments;
mod bulk;
mod files;
mod maintenance;

pub(crate) use attachments::{draft_attachment_path, ensure_attachment_data};
pub(crate) use files::file_url;

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

/// Persist a local read/star change. `""` on success, else the error for the
/// status line -- a swallowed failure would report "Marked as read" while the
/// flag silently stayed put (the rebuilt feed shows the unchanged DB state).
fn save_flags(db: &mailcore::Db, message_id: i64, read: bool, starred: bool) -> QString {
    match messages::set_flags(db, message_id, read, starred) {
        Ok(_) => qstring(""),
        Err(e) => {
            log::warn!("flags: message {message_id} not updated: {e}");
            qstring(&format!("could not update the message: {e}"))
        }
    }
}

impl qobject::Bridge {
    pub fn message_html(&self, uid: i32, allow_remote: bool) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("");
        }
        feed::message_html(db, folder_id, uid as u32, allow_remote)
            .map_or_else(|_| qstring(""), |h| qstring(&h))
    }

    pub fn message_json(&self, uid: i32) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("{}");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("{}");
        }
        feed::message_json(db, folder_id, uid as u32)
            .map_or_else(|_| qstring("{}"), |json| qstring(&json))
    }

    pub fn attachments_json(&self, uid: i32) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("[]");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("[]");
        }
        feed::attachments_json(db, folder_id, uid as u32)
            .map_or_else(|_| qstring("[]"), |j| qstring(&j))
    }

    pub fn message_headers_json(&self, uid: i32) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("{}");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("{}");
        }
        feed::headers_json(db, folder_id, uid as u32)
            .map_or_else(|_| qstring("{}"), |j| qstring(&j))
    }

    pub fn answer_draft_json(&self, uid: i32, mode: &QString) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("{}");
        };
        let folder_id = *self.current_folder_id();
        if folder_id < 0 || uid < 0 {
            return qstring("{}");
        }
        mailcore::compose::answer_draft_json(db, folder_id, uid as u32, &mode.to_string())
            .map_or_else(|_| qstring("{}"), |j| qstring(&j))
    }

    pub fn blank_draft_json(&self) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("{}");
        };
        mailcore::compose::blank_draft_json(db).map_or_else(|_| qstring("{}"), |j| qstring(&j))
    }

    /// FTS search for the toolbar (up to 50 hits, rank order). `folder`
    /// scopes to one folder path (empty = whole account). Local SQLite
    /// read, no network — safe to call per keystroke.
    pub fn search_json(&self, query: &QString, folder: &QString) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("[]");
        };
        let acc_id = *self.current_account_id();
        if acc_id < 0 {
            return qstring("[]");
        }
        feed::search_json(db, acc_id, &query.to_string(), 50, &folder.to_string())
            .map_or_else(|_| qstring("[]"), |j| qstring(&j))
    }

    pub fn open_attachment(self: Pin<&mut Self>, attachment_id: i32) -> QString {
        if attachment_id < 0 {
            return qstring("unknown attachment");
        }
        spawn_job(self, "Open", move |db, _progress| async move {
            let parent =
                messages::get_attachment(db, attachment_id as i64).map_err(|e| e.to_string())?;
            ensure_attachment_data(db, parent.message_id, false).await?;
            let dir = std::env::temp_dir().join("mailclient-attachments");
            let dest = messages::write_attachment_copy(db, attachment_id as i64, &dir)
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
                messages::get_attachment(db, attachment_id as i64).map_err(|e| e.to_string())?;
            ensure_attachment_data(db, parent.message_id, false).await?;
            let dest = messages::save_attachment_to(db, attachment_id as i64, &path)
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
            let msg = messages::get_by_uid(db, folder_id, uid as u32)
                .map_err(|_| "unknown message".to_string())?;
            ensure_attachment_data(db, msg.id, false).await?;
            let saved =
                messages::save_all_attachments_to(db, msg.id, &dir).map_err(|e| e.to_string())?;
            Ok((format!("Saved {saved} attachment(s)"), None))
        })
    }

    pub fn open_message(mut self: Pin<&mut Self>, uid: i32) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let Ok(msg) = messages::get_by_uid(db, folder_id, uid as u32) else {
            return qstring("");
        };
        let mut result = qstring("");
        if !msg.is_read {
            result = save_flags(db, msg.id, true, msg.is_starred);
            // Seen is pushed promptly in the background (plus again on the
            // next sync), so closing the app right after reading loses nothing.
            spawn_flag_push(acc_id);
        }
        // Refresh the QML-bound feeds so the follow-up reloadMessages() /
        // reloadFolders() in QML see the cleared unread flag immediately.
        // Without this messages_json/folders_json stay stale and the marker
        // only clears on the next folder switch (which pushes feeds).
        push_feeds(&mut self, db, acc_id, folder_id);
        result
    }

    pub fn mark_read(mut self: Pin<&mut Self>, uid: i32, read: bool) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let Ok(msg) = messages::get_by_uid(db, folder_id, uid as u32) else {
            return qstring("message is no longer available");
        };
        // Queued locally, pushed promptly in the background (see open_message).
        let result = save_flags(db, msg.id, read, msg.is_starred);
        spawn_flag_push(acc_id);
        push_feeds(&mut self, db, acc_id, folder_id);
        result
    }

    pub fn toggle_star(mut self: Pin<&mut Self>, uid: i32) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        let Ok(msg) = messages::get_by_uid(db, folder_id, uid as u32) else {
            return qstring("message is no longer available");
        };
        // Queued locally, pushed promptly in the background (see open_message).
        let result = save_flags(db, msg.id, msg.is_read, !msg.is_starred);
        spawn_flag_push(acc_id);
        push_feeds(&mut self, db, acc_id, folder_id);
        result
    }

    pub fn delete_message(self: Pin<&mut Self>, uid: i32) -> QString {
        self.queue_undoable(vec![uid as u32], MoveTarget::Trash)
    }

    pub fn archive_message(self: Pin<&mut Self>, uid: i32) -> QString {
        self.queue_undoable(vec![uid as u32], MoveTarget::Archive)
    }

    pub fn move_message(self: Pin<&mut Self>, uid: i32, path: &QString) -> QString {
        self.queue_undoable(vec![uid as u32], MoveTarget::Folder(path.to_string()))
    }

    pub fn purge_message(self: Pin<&mut Self>, uid: i32) -> QString {
        self.purge_uids(vec![uid as u32])
    }

    pub fn set_sort(mut self: Pin<&mut Self>, field: &QString, descending: bool) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let normalized = settings::normalize_sort_field(&field.to_string()).to_string();
        if let Err(e) = settings::set_sort(db, &normalized, descending) {
            return qstring(&e.to_string());
        }
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        // push_feeds re-reads the settings for the feed and syncs the props.
        push_feeds(&mut self, db, acc_id, folder_id);
        qstring("")
    }
}
