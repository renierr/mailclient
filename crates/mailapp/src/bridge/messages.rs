use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::feed;
use mailcore::models::FolderRole;
use mailcore::store::{accounts, folders, messages, settings};
use mailcore::sync::imap::{ArchiveOutcome, MoveOutcome, TrashOutcome};

use crate::bridge::qobject;
use crate::bridge::session::{current_account, guard_sync, with_imap};
use crate::bridge::{open_db, push_feeds, qstring, MAX_MESSAGE_LIMIT};

/// Strip a `file://` URL prefix from save-dialog output into a plain path.
pub(crate) fn dir_to_path(raw: &str) -> std::path::PathBuf {
    let t = raw.trim();
    let stripped = t.strip_prefix("file://").unwrap_or(t);
    std::path::PathBuf::from(stripped)
}

/// Absolute path → `file://` URL for `Qt.openUrlExternally`. Percent-encodes
/// everything but `/` and unreserved characters so spaces, `#` and non-ASCII
/// names survive the QML string→QUrl conversion.
pub(crate) fn file_url(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::with_capacity(s.len() + 7);
    out.push_str("file://");
    // A bare `C:/…` would parse as host `C:` — anchor it as an empty host.
    if !(s.starts_with('/') || s.starts_with("file:")) {
        out.push('/');
    }
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Filename safe for the filesystem: keeps the basename, replaces path
/// separators, falls back to `attachment-<id>.bin`.
pub(crate) fn safe_filename(name: Option<&str>, id: i64) -> String {
    let base = name
        .unwrap_or("")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    if base.is_empty() {
        return format!("attachment-{id}.bin");
    }
    base.replace(['/', '\\', '\0'], "_")
}

/// `photo.pdf` + 1 → `photo(1).pdf` (save-all collision avoidance).
pub(crate) fn numbered_filename(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{}({n}).{}", &name[..i], &name[i + 1..]),
        _ => format!("{name}({n})"),
    }
}

/// Resolve the save-dialog target for one attachment: `file://` tolerant,
/// directories auto-append the attachment filename, parents created.
pub(crate) fn resolve_save_path(
    db: &mailcore::Db,
    attachment_id: i64,
    raw: &str,
) -> Result<std::path::PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("choose where to save".to_string());
    }
    let mut p = dir_to_path(trimmed);
    if p.is_dir() || trimmed.ends_with('/') {
        let a = messages::get_attachment(db, attachment_id).map_err(|e| e.to_string())?;
        p.push(safe_filename(a.filename.as_deref(), attachment_id));
    }
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create folder: {e}"))?;
        }
    }
    Ok(p)
}

/// Make sure a message's file bytes are cached, downloading them now on
/// explicit user request. Background sync stores names/sizes only, so this
/// is the single place attachment bytes cross the network. Returns the
/// number of files downloaded (0 = already cached). Draft opening includes
/// inline parts because Composer must preserve them on replacement.
pub(crate) fn ensure_attachment_data(
    db: &mailcore::Db,
    message_id: i64,
    include_inline: bool,
) -> Result<u64, String> {
    let files = messages::list_attachments(db, message_id).map_err(|e| e.to_string())?;
    let mut missing = false;
    for a in files.iter().filter(|a| include_inline || !a.is_inline) {
        let has = messages::attachment_has_data(db, a.id).map_err(|e| e.to_string())?;
        if !has {
            missing = true;
            break;
        }
    }
    if !missing {
        return Ok(0);
    }
    let msg = messages::get(db, message_id).map_err(|e| e.to_string())?;
    let folder = folders::get(db, msg.folder_id).map_err(|e| e.to_string())?;
    let acc = accounts::get(db, folder.account_id).map_err(|e| e.to_string())?;
    with_imap(&acc, |imap| {
        imap.fetch_attachments(db, message_id)
            .map_err(|e| e.to_string())
    })
}

/// Materialize one cached attachment for Composer. The file name keeps the
/// original extension for MIME guessing. Each invocation owns a 0700 temp
/// directory, avoiding shared-temp name collisions.
pub(crate) fn draft_attachment_path(
    db: &mailcore::Db,
    attachment_id: i64,
) -> Result<String, String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);
    let attachment = messages::get_attachment(db, attachment_id).map_err(|e| e.to_string())?;
    let base = std::env::temp_dir();
    let unique = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let dir = base.join(format!("mailclient-draft-{}-{unique}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| format!("cannot create temp folder: {e}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|e| format!("cannot secure temp folder: {e}"))?;
    let name = format!(
        "{}-{}-{}",
        attachment.message_id,
        attachment.id,
        safe_filename(attachment.filename.as_deref(), attachment.id)
    );
    let dest = dir.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(|e| format!("cannot create draft attachment: {e}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(&dest, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|e| format!("cannot secure draft attachment: {e}"))?;
    let bytes = match attachment.data {
        Some(bytes) if !bytes.is_empty() => bytes,
        _ => {
            let source = attachment
                .storage_path
                .ok_or_else(|| "draft attachment has no cached data".to_string())?;
            std::fs::read(source).map_err(|e| format!("cannot read draft attachment: {e}"))?
        }
    };
    file.write_all(&bytes)
        .map_err(|e| format!("cannot write draft attachment: {e}"))?;
    Ok(file_url(&dest))
}

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

    pub fn open_attachment(&self, attachment_id: i32) -> QString {
        if attachment_id < 0 {
            return qstring("unknown attachment");
        }
        // Guarded: ensuring bytes may hit the network on first open.
        let result = guard_sync("Open", || {
            let db = open_db()?;
            let parent =
                messages::get_attachment(&db, attachment_id as i64).map_err(|e| e.to_string())?;
            // Open implies download: fetch bytes when not cached yet.
            ensure_attachment_data(&db, parent.message_id, false)?;
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
            Ok(file_url(&dest))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn save_attachment(&self, attachment_id: i32, path: &QString) -> QString {
        if attachment_id < 0 {
            return qstring("unknown attachment");
        }
        // Guarded: ensuring bytes may hit the network on first save.
        let result = guard_sync("Save", || {
            let db = open_db()?;
            let parent =
                messages::get_attachment(&db, attachment_id as i64).map_err(|e| e.to_string())?;
            ensure_attachment_data(&db, parent.message_id, false)?;
            let dest = resolve_save_path(&db, attachment_id as i64, &path.to_string())?;
            messages::save_attachment_to_path(&db, attachment_id as i64, &dest)
                .map_err(|e| e.to_string())?;
            Ok(format!("Saved to {}", dest.display()))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn save_all_attachments(&self, uid: i32, dir: &QString) -> QString {
        // Guarded: ensuring bytes may hit the network on first save.
        let folder_id = *self.current_folder_id();
        let result = guard_sync("Save", || {
            let db = open_db()?;
            if folder_id < 0 || uid < 0 {
                return Err("no message selected".to_string());
            }
            let msg = messages::get_by_uid(&db, folder_id, uid as u32)
                .map_err(|_| "unknown message".to_string())?;
            // Download on explicit request only — then save from the cache.
            ensure_attachment_data(&db, msg.id, false)?;
            let files = messages::list_attachments(&db, msg.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|a| !a.is_inline)
                .collect::<Vec<_>>();
            if files.is_empty() {
                return Err("no attachments to save".to_string());
            }
            let mut base = dir_to_path(&dir.to_string());
            std::fs::create_dir_all(&base).map_err(|e| format!("cannot create folder: {e}"))?;
            let mut saved = 0u32;
            for a in &files {
                let name = safe_filename(a.filename.as_deref(), a.id);
                base.push(&name);
                // Never overwrite: photo(1).pdf, photo(2).pdf, …
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
                Err("could not save attachments".to_string())
            } else {
                Ok(format!("Saved {saved} attachment(s)"))
            }
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
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
            // Local write + dirty mark only. Pushing \Seen here meant a full
            // IMAP connect on every click, which froze the list and made
            // selection appear stuck; `sync_now` flushes the queue instead.
            let _ = messages::set_flags(&db, msg.id, true, msg.is_starred);
        }
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
        // Queued, not pushed: see open_message.
        let _ = messages::set_flags(&db, msg.id, read, msg.is_starred);
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
        // Queued, not pushed: see open_message.
        let _ = messages::set_flags(&db, msg.id, msg.is_read, !msg.is_starred);
        push_feeds(&mut self, &db, acc_id, folder_id);
        qstring("")
    }

    pub fn delete_message(mut self: Pin<&mut Self>, uid: i32) -> QString {
        // Guarded: any panic becomes a status message, never SIGABRT.
        let result = guard_sync("Delete", || {
            let db = open_db()?;
            let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
            let msg =
                messages::get_by_uid(&db, folder_id, uid as u32).map_err(|_| String::new())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let outcome = with_imap(&acc, |imap| {
                imap.trash_message(&db, msg.id).map_err(|e| e.to_string())
            })?;
            push_feeds(&mut self, &db, acc_id, folder_id);
            // Reported, not silent: "deleted permanently" is a different promise
            // from "moved to Trash" and the user needs to know which happened.
            Ok(match outcome {
                TrashOutcome::Moved(path) => format!("Moved to {path}"),
                TrashOutcome::Expunged => "Deleted permanently".to_string(),
            })
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn archive_message(mut self: Pin<&mut Self>, uid: i32) -> QString {
        // Guarded: any panic becomes a status message, never SIGABRT.
        let result = guard_sync("Archive", || {
            let db = open_db()?;
            let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
            let msg =
                messages::get_by_uid(&db, folder_id, uid as u32).map_err(|_| String::new())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let outcome = with_imap(&acc, |imap| {
                imap.archive_message(&db, msg.id).map_err(|e| e.to_string())
            })?;
            push_feeds(&mut self, &db, acc_id, folder_id);
            Ok(match outcome {
                ArchiveOutcome::Moved(path) => format!("Archived to {path}"),
                ArchiveOutcome::AlreadyThere => "Already in Archive".to_string(),
            })
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn move_message(mut self: Pin<&mut Self>, uid: i32, path: &QString) -> QString {
        // Guarded: any panic becomes a status message, never SIGABRT.
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let path = path.to_string();
        let result = guard_sync("Move", || {
            let db = open_db()?;
            let acc = current_account(&db, wanted)?;
            let msg = messages::get_by_uid(&db, current, uid as u32).map_err(|_| String::new())?;
            let dest = folders::get_by_path(&db, acc.id, &path).map_err(|e| e.to_string())?;
            let outcome = with_imap(&acc, |imap| {
                imap.move_to_folder(&db, msg.id, dest.id)
                    .map_err(|e| e.to_string())
            })?;
            push_feeds(&mut self, &db, acc.id, current);
            Ok(match outcome {
                MoveOutcome::Moved(path) => format!("Moved to {path}"),
                MoveOutcome::AlreadyThere => "Already here".to_string(),
            })
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn purge_message(mut self: Pin<&mut Self>, uid: i32) -> QString {
        // Guarded: any panic becomes a status message, never SIGABRT.
        let result = guard_sync("Delete", || {
            let db = open_db()?;
            let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
            let msg =
                messages::get_by_uid(&db, folder_id, uid as u32).map_err(|_| String::new())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            with_imap(&acc, |imap| {
                imap.delete_message(&db, msg.id).map_err(|e| e.to_string())
            })?;
            push_feeds(&mut self, &db, acc_id, folder_id);
            Ok("Deleted permanently".to_string())
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
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
                } else if n == 1 {
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
                } else if n == 1 {
                    qstring(if starred { "Starred 1" } else { "Unstarred 1" })
                } else if starred {
                    qstring(&format!("Starred {n}"))
                } else {
                    qstring(&format!("Unstarred {n}"))
                }
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn delete_many(mut self: Pin<&mut Self>, uids_json: &QString) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let result = guard_sync("Delete", || {
            let db = open_db()?;
            let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
            let folder = folders::get(&db, folder_id).map_err(|e| e.to_string())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            if folder.account_id != acc.id {
                return Err("folder does not belong to this account".to_string());
            }
            // Junk never touches Trash; Trash deletes are permanent — same
            // rule as the single-message path, applied once per folder.
            let summary = if folder.role == FolderRole::Junk {
                let n = with_imap(&acc, |imap| {
                    imap.purge_uids(&db, folder_id, &uids)
                        .map_err(|e| e.to_string())
                })?;
                format!("Deleted {n} permanently")
            } else {
                let trash = folders::list_by_account(&db, acc.id)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .find(|f| f.role == FolderRole::Trash);
                match trash.filter(|t| t.id != folder.id) {
                    Some(t) => {
                        let n = with_imap(&acc, |imap| {
                            imap.move_uids_to(&db, folder_id, &uids, &t.path)
                                .map_err(|e| e.to_string())
                        })?;
                        format!("Moved {n} to {}", t.path)
                    }
                    None => {
                        let n = with_imap(&acc, |imap| {
                            imap.purge_uids(&db, folder_id, &uids)
                                .map_err(|e| e.to_string())
                        })?;
                        format!("Deleted {n} permanently")
                    }
                }
            };
            push_feeds(&mut self, &db, acc_id, folder_id);
            Ok(summary)
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn archive_many(mut self: Pin<&mut Self>, uids_json: &QString) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let result = guard_sync("Archive", || {
            let db = open_db()?;
            let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
            let folder = folders::get(&db, folder_id).map_err(|e| e.to_string())?;
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let summary = with_imap(&acc, |imap| {
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
                            .map_err(|e| e.to_string())?
                    }
                };
                if archive.id == folder.id {
                    return Ok("Already in Archive".to_string());
                }
                let n = imap
                    .move_uids_to(&db, folder_id, &uids, &archive.path)
                    .map_err(|e| e.to_string())?;
                Ok(format!("Archived {n} to {}", archive.path))
            })?;
            push_feeds(&mut self, &db, acc_id, folder_id);
            Ok(summary)
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn move_many(mut self: Pin<&mut Self>, uids_json: &QString, path: &QString) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let path = path.to_string();
        let result = guard_sync("Move", || {
            let db = open_db()?;
            let acc = current_account(&db, wanted)?;
            let dest = folders::get_by_path(&db, acc.id, &path).map_err(|e| e.to_string())?;
            if dest.id == current {
                return Ok("Already here".to_string());
            }
            // Sanity: source folder must belong to this account.
            let folder = folders::get(&db, current).map_err(|e| e.to_string())?;
            if folder.account_id != acc.id {
                return Err("folder does not belong to this account".to_string());
            }
            let n = with_imap(&acc, |imap| {
                imap.move_uids_to(&db, current, &uids, &dest.path)
                    .map_err(|e| e.to_string())
            })?;
            push_feeds(&mut self, &db, acc.id, current);
            Ok(format!("Moved {n} to {}", dest.path))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn purge_many(mut self: Pin<&mut Self>, uids_json: &QString) -> QString {
        let uids = match parse_uids_json(&uids_json.to_string()) {
            Ok(u) => u,
            Err(e) => return qstring(&e),
        };
        let result = guard_sync("Delete", || {
            let db = open_db()?;
            let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
            let acc = accounts::get(&db, acc_id).map_err(|e| e.to_string())?;
            let n = with_imap(&acc, |imap| {
                imap.purge_uids(&db, folder_id, &uids)
                    .map_err(|e| e.to_string())
            })?;
            push_feeds(&mut self, &db, acc_id, folder_id);
            Ok(format!("Deleted {n} permanently"))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }
}
