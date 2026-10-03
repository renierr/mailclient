use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::feed;
use mailcore::html::reader;
use mailcore::store::{messages, settings};
use mailcore::sync::attachments::ensure_cached;
use mailcore::undo::MoveTarget;

use crate::bridge::qobject;
use crate::bridge::worker::{spawn_flag_push, spawn_job};
use crate::bridge::{push_feeds, qstring, shared_db, MAX_MESSAGE_LIMIT};

mod attachments;
mod bulk;
mod files;
mod maintenance;

pub(crate) use attachments::draft_attachment_path;
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

/// Parse search-hit targets (`[{"folder": path, "uid": n}, ...]` from QML)
/// into per-folder UID lists, in first-seen folder order. Same caps and
/// dedup as [`parse_uids_json`], applied to the whole selection.
pub(crate) fn parse_hits_json(raw: &str) -> Result<Vec<(String, u32)>, String> {
    let invalid = || "invalid selection".to_string();
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|_| invalid())?;
    let arr = v.as_array().ok_or_else(invalid)?;
    if arr.is_empty() {
        return Err("no messages selected".to_string());
    }
    if arr.len() > MAX_MESSAGE_LIMIT as usize {
        return Err(format!(
            "too many messages selected (max {})",
            MAX_MESSAGE_LIMIT
        ));
    }
    let mut hits = Vec::with_capacity(arr.len());
    for x in arr {
        let folder = x
            .get("folder")
            .and_then(|f| f.as_str())
            .ok_or_else(invalid)?;
        let uid = x.get("uid").and_then(|u| u.as_u64()).ok_or_else(invalid)?;
        if folder.is_empty() || uid == 0 || uid > u64::from(u32::MAX) {
            return Err(invalid());
        }
        hits.push((folder.to_string(), uid as u32));
    }
    Ok(hits)
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

    /// FTS search for the toolbar (up to 50 hits, newest first). `folder`
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
        feed::search_json(
            db,
            acc_id,
            &query.to_string(),
            mailcore::search::HIT_LIMIT,
            &folder.to_string(),
        )
        .map_or_else(|_| qstring("[]"), |j| qstring(&j))
    }

    /// The folder a QML call names: its path, or the open folder when the
    /// path is empty (the reader). `None` when the path does not resolve —
    /// never a silent fallback to another folder, whose uid would name a
    /// different message.
    fn folder_for_path(&self, db: &mailcore::Db, folder_path: &QString) -> Option<i64> {
        let acc_id = *self.current_account_id();
        let folder_id = if folder_path.is_empty() {
            *self.current_folder_id()
        } else {
            mailcore::store::folders::get_by_path(db, acc_id, &folder_path.to_string())
                .ok()?
                .id
        };
        (acc_id >= 0 && folder_id >= 0).then_some(folder_id)
    }

    pub fn find_similar_json(&self, folder_path: &QString, uid: i32) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("[]");
        };
        let Some(folder_id) = self.folder_for_path(db, folder_path).filter(|_| uid >= 0) else {
            return qstring("[]");
        };
        mailcore::similar::similar_json(
            db,
            *self.current_account_id(),
            folder_id,
            uid as i64,
            mailcore::search::HIT_LIMIT,
        )
        .map_or_else(|_| qstring("[]"), |j| qstring(&j))
    }

    pub fn find_similar_subject(&self, folder_path: &QString, uid: i32) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("");
        };
        let Some(folder_id) = self.folder_for_path(db, folder_path).filter(|_| uid >= 0) else {
            return qstring("");
        };
        mailcore::similar::target_subject(db, *self.current_account_id(), folder_id, uid as i64)
            .map_or_else(|_| qstring(""), |s| qstring(&s))
    }

    pub fn reader_paint(&self, colored: bool, dark: bool, keep_original: bool) -> QString {
        qstring(reader::paint_for(colored, dark, keep_original).as_str())
    }

    pub fn reader_palette_json(&self, paint: &QString, theme_json: &QString) -> QString {
        let theme =
            reader_theme(&serde_json::from_str(&theme_json.to_string()).unwrap_or_default());
        let palette = reader::palette(reader::Paint::parse(&paint.to_string()), &theme);
        qstring(&serde_json::to_string(&palette).unwrap_or_else(|_| "{}".to_string()))
    }

    pub fn reader_fit_below(&self, body: &QString) -> i32 {
        i32::try_from(reader::fit_below(&body.to_string())).unwrap_or(i32::MAX)
    }

    pub fn reader_document(&self, body: &QString, options_json: &QString) -> QString {
        let o: serde_json::Value =
            serde_json::from_str(&options_json.to_string()).unwrap_or_default();
        let flag = |k: &str| {
            o.get(k)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        let extra_css = o
            .get("extra_css")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let options = reader::DocumentOptions {
            paint: reader::Paint::parse(
                o.get("paint").and_then(|v| v.as_str()).unwrap_or_default(),
            ),
            theme: reader_theme(o.get("theme").unwrap_or(&serde_json::Value::Null)),
            allow_remote: flag("allow_remote"),
            top_space: o
                .get("top_space")
                .and_then(serde_json::Value::as_f64)
                .map_or(0, |v| v.max(0.0).ceil() as u32),
            scale: o
                .get("scale")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(1.0) as f32,
            fit: flag("fit"),
            extra_css,
        };
        qstring(&reader::document(&body.to_string(), &options))
    }

    pub fn link_info_json(&self, url: &QString) -> QString {
        let info = mailcore::html::link_info(&url.to_string());
        qstring(&serde_json::to_string(&info).unwrap_or_else(|_| "{}".to_string()))
    }

    pub fn search_syntax_help(&self) -> QString {
        qstring(mailcore::search::SYNTAX_HELP)
    }

    pub fn search_plan_json(&self, query: &QString) -> QString {
        let plan = mailcore::search::plan(&query.to_string());
        qstring(&serde_json::to_string(&plan).unwrap_or_else(|_| "{}".to_string()))
    }

    pub fn search_filter_matches(
        &self,
        query: &QString,
        subject: &QString,
        from: &QString,
        from_name: &QString,
        snippet: &QString,
    ) -> bool {
        mailcore::search::filter_matches(
            &query.to_string(),
            &subject.to_string(),
            &from.to_string(),
            &from_name.to_string(),
            &snippet.to_string(),
        )
    }

    pub fn open_attachment(self: Pin<&mut Self>, attachment_id: i32) -> QString {
        if attachment_id < 0 {
            return qstring("unknown attachment");
        }
        spawn_job(self, "Open", move |db, _progress| async move {
            let parent =
                messages::get_attachment(db, attachment_id as i64).map_err(|e| e.to_string())?;
            ensure_cached(db, parent.message_id, false).await?;
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
            ensure_cached(db, parent.message_id, false).await?;
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
            ensure_cached(db, msg.id, false).await?;
            let saved =
                messages::save_all_attachments_to(db, msg.id, &dir).map_err(|e| e.to_string())?;
            Ok((format!("Saved {saved} attachment(s)"), None))
        })
    }

    pub fn suggested_eml_name(&self, folder_path: &QString, uid: i32) -> QString {
        let fallback = || qstring(&format!("message-{uid}.eml"));
        let Ok(db) = shared_db() else {
            return fallback();
        };
        match self.folder_for_path(db, folder_path).filter(|_| uid >= 0) {
            Some(folder_id) => qstring(&mailcore::export::suggested_eml_name(
                db, folder_id, uid as u32,
            )),
            None => fallback(),
        }
    }

    pub fn export_message(
        self: Pin<&mut Self>,
        folder_path: &QString,
        uid: i32,
        path: &QString,
    ) -> QString {
        let folder_id = shared_db()
            .ok()
            .and_then(|db| self.folder_for_path(db, folder_path))
            .filter(|_| uid >= 0);
        let path = path.to_string();
        spawn_job(self, "Export", move |db, _progress| async move {
            let folder_id = folder_id.ok_or_else(|| "unknown message".to_string())?;
            mailcore::export::prepare(db, folder_id, uid as u32).await?;
            let dest = mailcore::export::export_eml_to(db, folder_id, uid as u32, &path)
                .map_err(|e| e.to_string())?;
            Ok((format!("Exported to {}", dest.display()), None))
        })
    }

    pub fn mark_read_plan_json(&self, unread: bool) -> QString {
        let (auto, delay) = match shared_db() {
            Ok(db) => (
                settings::get_bool(db, settings::AUTO_MARK_READ).unwrap_or(true),
                settings::get_delay_secs(db, settings::MARK_READ_DELAY_SECS),
            ),
            Err(_) => (false, 0),
        };
        let (plan, delay_secs) = match settings::mark_read_plan(auto, delay, unread) {
            settings::MarkReadPlan::Off => ("off", 0),
            settings::MarkReadPlan::Now => ("now", 0),
            settings::MarkReadPlan::AfterDelay(s) => ("after", s),
        };
        qstring(&format!(
            "{{\"plan\":\"{plan}\",\"delay_secs\":{delay_secs}}}"
        ))
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

/// The theme colours QML sends (`{paper, ink, link, quote, rule}` as
/// `#rrggbb`); a missing or unreadable one keeps the light sheet's.
fn reader_theme(v: &serde_json::Value) -> reader::Palette {
    let css = |k: &str| v.get(k).and_then(|c| c.as_str()).unwrap_or_default();
    reader::Palette::from_css(
        css("paper"),
        css("ink"),
        css("link"),
        css("quote"),
        css("rule"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hits_parse_as_sent_and_group_in_the_core() {
        let hits = parse_hits_json(r#"[{"folder":"Archive","uid":7},{"folder":"INBOX","uid":3}]"#)
            .unwrap();
        assert_eq!(
            hits,
            vec![("Archive".to_string(), 7), ("INBOX".to_string(), 3)]
        );
    }

    #[test]
    fn hits_reject_malformed_targets() {
        assert!(parse_hits_json("[]").is_err());
        assert!(parse_hits_json("[3]").is_err());
        assert!(parse_hits_json(r#"[{"folder":"","uid":3}]"#).is_err());
        assert!(parse_hits_json(r#"[{"folder":"INBOX","uid":0}]"#).is_err());
    }
}
