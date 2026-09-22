//! Network sync jobs. Every one of these queues onto `mailclient-net` and
//! reports back on the event stream; none of them block the caller.

use mailcore::store::folders;
use mailcore::sync::headless;
use mailcore::sync::imap::{FULL_SYNC_WINDOW, OLDER_BATCH};
use mailcore::sync::traits::SyncProvider;

use crate::net::{spawn, JobRefresh};
use crate::session::{checkout_session, resolve_account};

/// Full sync for one account: outbox flush, dirty-flag push, folder sweep.
///
/// Selective and windowed, exactly as the Qt frontend runs it — the folder
/// LIST is always cheap, the inbox syncs its newest window fully, and every
/// other folder only refreshes flags plus a short window, so the sidebar
/// pills stay honest without downloading the whole mailbox. Use
/// [`sync_folder_now`] to fill a folder the user actually opened.
pub fn sync_account(account_id: i64) -> anyhow::Result<()> {
    spawn(
        "Sync",
        format!("sync:{account_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            // The shared orchestration `mailapp` and the CLI both use; we only
            // lend it a pooled session.
            let mut imap = checkout_session(&acc).await?;
            let r = headless::sync_account(db, &acc, &mut imap).await;
            imap.checkin();

            let flags = match r.pushed_flags {
                0 => String::new(),
                n => format!(", {n} flag(s) pushed"),
            };
            let quick = r.folders.iter().filter(|f| f.role != "inbox").count();
            let scope = match quick {
                0 => String::new(),
                n => format!(" (inbox full, {n} folder(s) quick)"),
            };
            let hidden = match r.folders_skipped_hidden {
                0 => String::new(),
                n => format!(", {n} hidden skipped"),
            };
            let errs = match r.errors.first() {
                None => String::new(),
                Some(first) => format!("; {} error(s): {first}", r.errors.len()),
            };
            Ok((
                format!(
                    "Synced {} folders: +{} new, -{} removed{flags}{scope}{hidden}{errs}",
                    r.folders_synced, r.fetched, r.expunged,
                ),
                Some(JobRefresh::account(acc.id)),
            ))
        },
    )
}

/// Fill one folder properly (its newest full window), on open or on demand.
///
/// Not on the folder-click path: SELECT + UID SEARCH + body fetches are
/// synchronous, so clicking a folder stays cache-only and instant. This is
/// what the UI calls *after* it has painted the cached rows.
pub fn sync_folder(account_id: i64, folder_id: i64) -> anyhow::Result<()> {
    spawn(
        "Sync",
        format!("sync-folder:{folder_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            let folder = folders::get(db, folder_id).map_err(|e| e.to_string())?;
            if folder.account_id != acc.id {
                return Err("folder does not belong to this account".to_string());
            }
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
                Some(JobRefresh::folder(acc.id, folder.id)),
            ))
        },
    )
}

/// Fetch the next older batch for a folder, so the list extends backwards.
pub fn load_older_messages(account_id: i64, folder_id: i64) -> anyhow::Result<()> {
    spawn(
        "Sync",
        format!("older:{folder_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
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
            let status = match r.fetched {
                0 => "Caught up — no older messages on the server".to_string(),
                n => format!("Loaded {n} older messages"),
            };
            // Nothing fetched changed nothing, so say so and spare the re-read.
            let refresh = (r.fetched > 0).then(|| JobRefresh::folder(acc.id, folder_id));
            Ok((status, refresh))
        },
    )
}

/// Refresh only the folder LIST — no bodies. This is how new, renamed or
/// deleted server-side folders appear; cheap enough to run on every open of
/// the folder manager.
pub fn refresh_folders(account_id: i64) -> anyhow::Result<()> {
    spawn(
        "Folders",
        format!("folders:{account_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            let mut imap = checkout_session(&acc).await?;
            let list = imap
                .sync_folders(db, acc.id)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                format!("{} folders on the server", list.len()),
                Some(JobRefresh::account(acc.id)),
            ))
        },
    )
}

/// Ask an account's IMAP server what it can do (the About view).
///
/// The capability list arrives as the finishing event's `status`, JSON-encoded
/// — the one place a job's status line is a payload rather than prose.
pub fn refresh_server_capabilities(account_id: i64) -> anyhow::Result<()> {
    spawn(
        "Capabilities",
        format!("caps:{account_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            let mut imap = checkout_session(&acc).await?;
            let caps = imap.capabilities_list().await.map_err(|e| e.to_string())?;
            imap.checkin();
            let payload = serde_json::json!({
                "account_id": acc.id,
                "email": acc.email_address,
                "imap_host": acc.imap_host,
                "imap_port": acc.imap_port,
                "capabilities": caps,
            });
            Ok((payload.to_string(), None))
        },
    )
}
