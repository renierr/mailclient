use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::store::{folders, messages};
use mailcore::sync::imap::{FULL_SYNC_WINDOW, OLDER_BATCH, QUICK_SYNC_WINDOW};
use mailcore::sync::traits::SyncProvider;

use crate::bridge::qobject;
use crate::bridge::session::{current_account, drop_all_imap_sessions, guard_sync, with_imap};
use crate::bridge::{open_db, push_feeds, qstring, DEFAULT_MESSAGE_LIMIT};

impl qobject::Bridge {
    pub fn sync_now(mut self: Pin<&mut Self>) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let result = guard_sync("Sync", || {
            let acc = current_account(&db, wanted)?;
            let mut pushed = 0u64;
            let mut fetched = 0u64;
            let mut expunged = 0u64;
            let mut quick = 0usize;
            let mut skipped = 0usize;
            let folders = with_imap(&acc, |imap| {
                // Push locally queued read/star changes first, so the fetch below
                // cannot overwrite them with stale server flags.
                for m in messages::list_flags_dirty(&db, acc.id).unwrap_or_default() {
                    if imap.push_flags(&db, &m).is_ok() {
                        let _ = messages::clear_flags_dirty(&db, m.id);
                        pushed += 1;
                    }
                }
                let folders = imap.sync_folders(&db, acc.id).map_err(|e| e.to_string())?;
                // Selective: INBOX gets the full window (newest 200 full bodies),
                // every other *visible* folder only flags + newest 50. Hidden
                // (unsubscribed) folders are LISTed so they stay manageable, but
                // their bodies are skipped — open one explicitly and it syncs.
                // Custom folders never auto-sync all mail — they fill on demand.
                for f in &folders {
                    if !f.subscribed {
                        skipped += 1;
                        continue;
                    }
                    let window = if f.role == mailcore::models::FolderRole::Inbox {
                        Some(FULL_SYNC_WINDOW)
                    } else {
                        quick += 1;
                        Some(QUICK_SYNC_WINDOW)
                    };
                    let r = imap
                        .sync_folder_window(&db, f.id, window)
                        .map_err(|e| e.to_string())?;
                    fetched += r.fetched;
                    expunged += r.expunged;
                }
                Ok(folders)
            })?;
            // Keep selection if it still exists, else inbox, else first.
            let all = folders::list_by_account(&db, acc.id).map_err(|e| e.to_string())?;
            let current = *self.current_folder_id();
            let folder_id = all
                .iter()
                .find(|f| f.id == current)
                .or_else(|| {
                    all.iter()
                        .find(|f| f.role == mailcore::models::FolderRole::Inbox)
                })
                .or(all.first())
                .map(|f| f.id)
                .unwrap_or(-1);
            push_feeds(&mut self, &db, acc.id, folder_id);
            let flags = if pushed > 0 {
                format!(", {pushed} flag(s) pushed")
            } else {
                String::new()
            };
            let scope = if quick > 0 {
                format!(" (inbox full, {quick} folder(s) quick)")
            } else {
                String::new()
            };
            let hidden = if skipped > 0 {
                format!(", {skipped} hidden skipped")
            } else {
                String::new()
            };
            Ok(format!(
                "Synced {} folders: +{fetched} new, -{expunged} removed{flags}{scope}{hidden}",
                folders.len()
            ))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn sync_folder_now(mut self: Pin<&mut Self>, path: &QString) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let result = guard_sync("Sync", || {
            let acc = current_account(&db, wanted)?;
            let folder =
                folders::get_by_path(&db, acc.id, &path.to_string()).map_err(|e| e.to_string())?;
            // Flush pending flag pushes first so this folder's fetch cannot
            // revert a just-tapped read/star.
            let r = with_imap(&acc, |imap| {
                for m in messages::list_flags_dirty(&db, acc.id).unwrap_or_default() {
                    if imap.push_flags(&db, &m).is_ok() {
                        let _ = messages::clear_flags_dirty(&db, m.id);
                    }
                }
                imap.sync_folder_window(&db, folder.id, Some(FULL_SYNC_WINDOW))
                    .map_err(|e| e.to_string())
            })?;
            // Stay on the synced folder.
            push_feeds(&mut self, &db, acc.id, folder.id);
            Ok(format!(
                "Synced {}: +{} new, -{} removed",
                folder.path, r.fetched, r.expunged
            ))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn load_older_messages(mut self: Pin<&mut Self>) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        if folder_id < 0 {
            return qstring("no folder selected");
        }
        let result = guard_sync("Sync", || {
            let acc = current_account(&db, wanted)?;
            // Sanity: the folder must belong to this account.
            let folder = folders::get(&db, folder_id).map_err(|e| e.to_string())?;
            if folder.account_id != acc.id {
                return Err("folder does not belong to this account".to_string());
            }
            let r = with_imap(&acc, |imap| {
                imap.sync_older(&db, folder_id, OLDER_BATCH)
                    .map_err(|e| e.to_string())
            })?;
            if r.fetched > 0 {
                self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
            }
            push_feeds(&mut self, &db, acc.id, folder_id);
            if r.fetched > 0 {
                Ok(format!("Loaded {} older messages", r.fetched))
            } else {
                Ok("Caught up — no older messages on the server".to_string())
            }
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn refresh_folders(mut self: Pin<&mut Self>) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let result = guard_sync("Sync", || {
            let acc = current_account(&db, wanted)?;
            let list = with_imap(&acc, |imap| {
                imap.sync_folders(&db, acc.id).map_err(|e| e.to_string())
            })?;
            let current = *self.current_folder_id();
            let still_there = list.iter().any(|f| f.id == current);
            let folder_id = if still_there {
                current
            } else {
                folders::list_by_account(&db, acc.id)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .find(|f| f.role == mailcore::models::FolderRole::Inbox)
                    .map(|f| f.id)
                    .unwrap_or(-1)
            };
            push_feeds(&mut self, &db, acc.id, folder_id);
            Ok(format!("Found {} IMAP folders", list.len()))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn set_folder_subscribed(
        mut self: Pin<&mut Self>,
        path: &QString,
        subscribed: bool,
    ) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let acc = match current_account(&db, wanted) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        let folder = match folders::get_by_path(&db, acc.id, &path.to_string()) {
            Ok(f) => f,
            Err(e) => return qstring(&e.to_string()),
        };
        if let Err(e) = folders::set_subscribed(&db, folder.id, subscribed) {
            return qstring(&e.to_string());
        }
        let current = *self.current_folder_id();
        push_feeds(&mut self, &db, acc.id, current);
        qstring("")
    }

    pub fn select_folder(mut self: Pin<&mut Self>, path: &QString) -> QString {
        let db = match open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let acc = match current_account(&db, wanted) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        match folders::get_by_path(&db, acc.id, &path.to_string()) {
            Ok(f) => {
                // New folder context: restart paging from the first page.
                self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
                push_feeds(&mut self, &db, acc.id, f.id);
                qstring("")
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn create_folder(mut self: Pin<&mut Self>, path: &QString) -> QString {
        // Guarded: any panic becomes a status message, never SIGABRT.
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let path = path.to_string();
        let result = guard_sync("Sync", || {
            let db = open_db()?;
            let acc = current_account(&db, wanted)?;
            // Already known locally (same normalized path) = success.
            let delimiter = folders::list_by_account(&db, acc.id)
                .unwrap_or_default()
                .first()
                .map(|f| f.delimiter.clone())
                .unwrap_or_else(|| "/".to_string());
            let normalized =
                mailcore::sync::imap::normalize_folder_path(&path.to_string(), &delimiter)
                    .map_err(|e| e.to_string())?;
            if folders::get_by_path(&db, acc.id, &normalized).is_ok() {
                push_feeds(&mut self, &db, acc.id, current);
                return Ok("Folder already exists".to_string());
            }
            let folder = with_imap(&acc, |imap| {
                imap.create_folder_path(&db, acc.id, &normalized, &delimiter)
                    .map_err(|e| e.to_string())
                    .map(|f| f.path)
            })?;
            push_feeds(&mut self, &db, acc.id, current);
            Ok(format!("Created {folder}"))
        });
        match result {
            Ok(s) => qstring(&s),
            Err(e) => qstring(&e),
        }
    }

    pub fn disconnect_all(&self) {
        drop_all_imap_sessions();
    }
}
