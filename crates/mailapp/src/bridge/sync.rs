use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::store::folders;
use mailcore::sync::headless;
use mailcore::sync::imap::{FULL_SYNC_WINDOW, OLDER_BATCH};
use mailcore::sync::traits::SyncProvider;

use crate::bridge::qobject;
use crate::bridge::session::{checkout_session, current_account, drop_all_imap_sessions};
use crate::bridge::worker::{spawn_job, JobRefresh};
use crate::bridge::{push_feeds, qstring, shared_db, DEFAULT_MESSAGE_LIMIT};

impl qobject::Bridge {
    pub fn sync_now(self: Pin<&mut Self>) -> QString {
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        spawn_job(self, "Sync", move |db, _progress| async move {
            let acc = current_account(db, wanted)?;
            // Shared orchestration (outbox flush, flag push, folder sweep);
            // the GUI lends its pooled session, the CLI brings a fresh one.
            let mut imap = checkout_session(&acc).await?;
            let r = headless::sync_account(db, &acc, &mut imap).await;
            imap.checkin();
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
            let flags = if r.pushed_flags > 0 {
                format!(", {} flag(s) pushed", r.pushed_flags)
            } else {
                String::new()
            };
            let quick = r.folders.iter().filter(|f| f.role != "inbox").count();
            let scope = if quick > 0 {
                format!(" (inbox full, {quick} folder(s) quick)")
            } else {
                String::new()
            };
            let hidden = if r.folders_skipped_hidden > 0 {
                format!(", {} hidden skipped", r.folders_skipped_hidden)
            } else {
                String::new()
            };
            let errs = if r.errors.is_empty() {
                String::new()
            } else {
                format!("; {} error(s): {}", r.errors.len(), r.errors[0])
            };
            Ok((
                format!(
                    "Synced {} folders: +{} new, -{} removed{flags}{scope}{hidden}{errs}",
                    r.folders_synced, r.fetched, r.expunged,
                ),
                Some(JobRefresh::feeds(acc.id, folder_id)),
            ))
        })
    }

    pub fn sync_folder_now(self: Pin<&mut Self>, path: &QString) -> QString {
        let wanted = *self.current_account_id();
        let path = path.to_string();
        spawn_job(self, "Sync", move |db, _progress| async move {
            let acc = current_account(db, wanted)?;
            let folder = folders::get_by_path(db, acc.id, &path).map_err(|e| e.to_string())?;
            let mut imap = checkout_session(&acc).await?;
            imap.push_dirty_flags(db, acc.id).await;
            let r = imap
                .sync_folder_window(db, folder.id, Some(FULL_SYNC_WINDOW))
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                format!(
                    "Synced {}: +{} new, -{} removed",
                    folder.path, r.fetched, r.expunged
                ),
                Some(JobRefresh::feeds(acc.id, folder.id)),
            ))
        })
    }

    pub fn load_older_messages(self: Pin<&mut Self>) -> QString {
        let wanted = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        if folder_id < 0 {
            return qstring("no folder selected");
        }
        spawn_job(self, "Sync", move |db, _progress| async move {
            let acc = current_account(db, wanted)?;
            let folder = folders::get(db, folder_id).map_err(|e| e.to_string())?;
            if folder.account_id != acc.id {
                return Err("folder does not belong to this account".to_string());
            }
            let mut imap = checkout_session(&acc).await?;
            let r = imap
                .sync_older(db, folder_id, OLDER_BATCH)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            let status = if r.fetched > 0 {
                format!("Loaded {} older messages", r.fetched)
            } else {
                "Caught up — no older messages on the server".to_string()
            };
            Ok((
                status,
                Some(JobRefresh {
                    account_id: acc.id,
                    folder_id,
                    message_limit: if r.fetched > 0 {
                        Some(DEFAULT_MESSAGE_LIMIT)
                    } else {
                        None
                    },
                }),
            ))
        })
    }

    pub fn refresh_folders(self: Pin<&mut Self>) -> QString {
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        spawn_job(self, "Sync", move |db, _progress| async move {
            let acc = current_account(db, wanted)?;
            let mut imap = checkout_session(&acc).await?;
            let list = imap
                .sync_folders(db, acc.id)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
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
                Some(JobRefresh::feeds(acc.id, folder_id)),
            ))
        })
    }

    pub fn search_server(self: Pin<&mut Self>, query: &QString, folder: &QString) -> QString {
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let query = query.to_string();
        let folder = folder.to_string();
        spawn_job(self, "Search", move |db, _progress| async move {
            let acc = current_account(db, wanted)?;
            let tokens = mailcore::search::search_tokens(&query);
            if tokens.is_empty() {
                return Ok(("Search: nothing searchable in that query".to_string(), None));
            }
            let scope = (!folder.is_empty()).then_some(folder.as_str());
            let mut imap = checkout_session(&acc).await?;
            let r = imap
                .search_server_into_cache(db, acc.id, &tokens, scope)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                if r.fetched > 0 {
                    format!(
                        "Server search: +{} message(s) in {} folder(s)",
                        r.fetched, r.folders_searched,
                    )
                } else {
                    "Server search: nothing more on the server".to_string()
                },
                // Backfilled mail changes counts/unread pills: rebuild the
                // feeds (and the search re-query on completion picks it up).
                Some(JobRefresh::feeds(acc.id, current)),
            ))
        })
    }

    pub fn set_folder_subscribed(
        mut self: Pin<&mut Self>,
        path: &QString,
        subscribed: bool,
    ) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let acc = match current_account(db, wanted) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        let folder = match folders::get_by_path(db, acc.id, &path.to_string()) {
            Ok(f) => f,
            Err(e) => return qstring(&e.to_string()),
        };
        if let Err(e) = folders::set_subscribed(db, folder.id, subscribed) {
            return qstring(&e.to_string());
        }
        let current = *self.current_folder_id();
        push_feeds(&mut self, db, acc.id, current);
        qstring("")
    }

    pub fn select_folder(mut self: Pin<&mut Self>, path: &QString) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let wanted = *self.current_account_id();
        let acc = match current_account(db, wanted) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        match folders::get_by_path(db, acc.id, &path.to_string()) {
            Ok(f) => {
                // New folder context: restart paging from the first page.
                self.as_mut().set_message_limit(DEFAULT_MESSAGE_LIMIT);
                push_feeds(&mut self, db, acc.id, f.id);
                qstring("")
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn create_folder(self: Pin<&mut Self>, path: &QString) -> QString {
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        let path = path.to_string();
        spawn_job(self, "Sync", move |db, _progress| async move {
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
                    Some(JobRefresh::feeds(acc.id, current)),
                ));
            }
            let mut imap = checkout_session(&acc).await?;
            let folder = imap
                .create_folder_path(db, acc.id, &normalized, &delimiter)
                .await
                .map_err(|e| e.to_string())
                .map(|f| f.path)?;
            imap.checkin();
            Ok((
                format!("Created {folder}"),
                Some(JobRefresh::feeds(acc.id, current)),
            ))
        })
    }

    pub fn disconnect_all(&self) {
        drop_all_imap_sessions();
    }
}
