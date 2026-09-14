use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::auth;
use mailcore::store::{folders, messages};
use mailcore::sync::imap::{FULL_SYNC_WINDOW, OLDER_BATCH, QUICK_SYNC_WINDOW};
use mailcore::sync::sender::SmtpSender;
use mailcore::sync::traits::SyncProvider;

use crate::bridge::qobject;
use crate::bridge::session::{current_account, drop_all_imap_sessions, with_imap};
use crate::bridge::worker::{spawn_job, JobRefresh};
use crate::bridge::{open_db, push_feeds, qstring, DEFAULT_MESSAGE_LIMIT};

impl qobject::Bridge {
    pub fn sync_now(self: Pin<&mut Self>) -> QString {
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        spawn_job(self, "Sync", move |db| {
            let acc = current_account(db, wanted)?;
            if let Ok(secrets) = auth::load_account_secrets(&acc.auth_vault_key) {
                let sender = SmtpSender::new(&acc);
                match sender.flush_outbox(db, acc.id, &secrets.smtp_password) {
                    Ok(n) if n > 0 => log::info!("smtp: flushed {n} queued send(s)"),
                    Err(e) => log::warn!("smtp: outbox flush failed: {e}"),
                    _ => {}
                }
            }
            let mut pushed = 0u64;
            let mut fetched = 0u64;
            let mut expunged = 0u64;
            let mut quick = 0usize;
            let mut skipped = 0usize;
            let folders = with_imap(&acc, |imap| {
                for m in messages::list_flags_dirty(db, acc.id).unwrap_or_default() {
                    if imap.push_flags(db, &m).is_ok() {
                        let _ = messages::clear_flags_dirty(db, m.id);
                        pushed += 1;
                    }
                }
                let folders = imap.sync_folders(db, acc.id).map_err(|e| e.to_string())?;
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
                        .sync_folder_window(db, f.id, window)
                        .map_err(|e| e.to_string())?;
                    fetched += r.fetched;
                    expunged += r.expunged;
                }
                Ok(folders)
            })?;
            let all = folders::list_by_account(db, acc.id).map_err(|e| e.to_string())?;
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
            Ok((
                format!(
                    "Synced {} folders: +{fetched} new, -{expunged} removed{flags}{scope}{hidden}",
                    folders.len()
                ),
                JobRefresh {
                    account_id: acc.id,
                    folder_id,
                    message_limit: None,
                },
            ))
        })
    }

    pub fn sync_folder_now(self: Pin<&mut Self>, path: &QString) -> QString {
        let wanted = *self.current_account_id();
        let path = path.to_string();
        spawn_job(self, "Sync", move |db| {
            let acc = current_account(db, wanted)?;
            let folder = folders::get_by_path(db, acc.id, &path).map_err(|e| e.to_string())?;
            let r = with_imap(&acc, |imap| {
                for m in messages::list_flags_dirty(db, acc.id).unwrap_or_default() {
                    if imap.push_flags(db, &m).is_ok() {
                        let _ = messages::clear_flags_dirty(db, m.id);
                    }
                }
                imap.sync_folder_window(db, folder.id, Some(FULL_SYNC_WINDOW))
                    .map_err(|e| e.to_string())
            })?;
            Ok((
                format!(
                    "Synced {}: +{} new, -{} removed",
                    folder.path, r.fetched, r.expunged
                ),
                JobRefresh {
                    account_id: acc.id,
                    folder_id: folder.id,
                    message_limit: None,
                },
            ))
        })
    }

    pub fn load_older_messages(self: Pin<&mut Self>) -> QString {
        let wanted = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        if folder_id < 0 {
            return qstring("no folder selected");
        }
        spawn_job(self, "Sync", move |db| {
            let acc = current_account(db, wanted)?;
            let folder = folders::get(db, folder_id).map_err(|e| e.to_string())?;
            if folder.account_id != acc.id {
                return Err("folder does not belong to this account".to_string());
            }
            let r = with_imap(&acc, |imap| {
                imap.sync_older(db, folder_id, OLDER_BATCH)
                    .map_err(|e| e.to_string())
            })?;
            let status = if r.fetched > 0 {
                format!("Loaded {} older messages", r.fetched)
            } else {
                "Caught up — no older messages on the server".to_string()
            };
            Ok((
                status,
                JobRefresh {
                    account_id: acc.id,
                    folder_id,
                    message_limit: if r.fetched > 0 {
                        Some(DEFAULT_MESSAGE_LIMIT)
                    } else {
                        None
                    },
                },
            ))
        })
    }

    pub fn refresh_folders(self: Pin<&mut Self>) -> QString {
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        spawn_job(self, "Sync", move |db| {
            let acc = current_account(db, wanted)?;
            let list = with_imap(&acc, |imap| {
                imap.sync_folders(db, acc.id).map_err(|e| e.to_string())
            })?;
            let still_there = list.iter().any(|f| f.id == current);
            let folder_id = if still_there {
                current
            } else {
                folders::list_by_account(db, acc.id)
                    .map_err(|e| e.to_string())?
                    .iter()
                    .find(|f| f.role == mailcore::models::FolderRole::Inbox)
                    .map(|f| f.id)
                    .unwrap_or(-1)
            };
            Ok((
                format!("Found {} IMAP folders", list.len()),
                JobRefresh {
                    account_id: acc.id,
                    folder_id,
                    message_limit: None,
                },
            ))
        })
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

    pub fn create_folder(self: Pin<&mut Self>, path: &QString) -> QString {
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let path = path.to_string();
        spawn_job(self, "Sync", move |db| {
            let acc = current_account(db, wanted)?;
            let delimiter = folders::list_by_account(db, acc.id)
                .unwrap_or_default()
                .first()
                .map(|f| f.delimiter.clone())
                .unwrap_or_else(|| "/".to_string());
            let normalized = mailcore::sync::imap::normalize_folder_path(&path, &delimiter)
                .map_err(|e| e.to_string())?;
            if folders::get_by_path(db, acc.id, &normalized).is_ok() {
                return Ok((
                    "Folder already exists".to_string(),
                    JobRefresh {
                        account_id: acc.id,
                        folder_id: current,
                        message_limit: None,
                    },
                ));
            }
            let folder = with_imap(&acc, |imap| {
                imap.create_folder_path(db, acc.id, &normalized, &delimiter)
                    .map_err(|e| e.to_string())
                    .map(|f| f.path)
            })?;
            Ok((
                format!("Created {folder}"),
                JobRefresh {
                    account_id: acc.id,
                    folder_id: current,
                    message_limit: None,
                },
            ))
        })
    }

    pub fn disconnect_all(&self) {
        drop_all_imap_sessions();
    }
}
