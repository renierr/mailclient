//! Server-side message moves and deletes.
//!
//! All of these take a *selection* — a list of UIDs — rather than having a
//! single-message and a bulk variant of each. The Qt bridge grew both, and
//! they drifted; a one-element list is the same operation, and the IMAP
//! commands underneath (`UID MOVE`, `UID STORE`+`UID EXPUNGE`) are set-shaped
//! anyway, so a hundred selected mails cost one round trip, not a hundred.

use mailcore::models::FolderRole;
use mailcore::store::folders;

use crate::net::{spawn, JobRefresh};
use crate::session::{checkout_session, resolve_account};

/// Delete a selection — which means Trash, except where it cannot.
///
/// Junk is destroyed outright (spam never passes through Trash), and so is
/// anything deleted from inside Trash itself or in an account that has no
/// Trash folder at all. The status line says which of the two happened,
/// because the difference is not recoverable.
pub fn delete_messages(account_id: i64, folder_id: i64, uids: Vec<u32>) -> anyhow::Result<()> {
    require_selection(&uids)?;
    spawn(
        "Delete",
        format!("delete:{folder_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            let folder = owned_folder(db, &acc, folder_id)?;
            let mut imap = checkout_session(&acc).await?;
            let trash = folders::list_by_account(db, acc.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|f| f.role == FolderRole::Trash)
                .filter(|t| t.id != folder.id);
            let summary = match trash.filter(|_| folder.role != FolderRole::Junk) {
                Some(t) => {
                    let n = imap
                        .move_uids_to(db, folder_id, &uids, &t.path)
                        .await
                        .map_err(|e| e.to_string())?;
                    format!("Moved {n} to {}", t.path)
                }
                None => {
                    let n = imap
                        .purge_uids(db, folder_id, &uids)
                        .await
                        .map_err(|e| e.to_string())?;
                    format!("Deleted {n} permanently")
                }
            };
            imap.checkin();
            Ok((summary, Some(JobRefresh::folder(acc.id, folder_id))))
        },
    )
}

/// Destroy a selection server-side (`\Deleted` + expunge). No undo — only for
/// an explicit "delete permanently".
pub fn purge_messages(account_id: i64, folder_id: i64, uids: Vec<u32>) -> anyhow::Result<()> {
    require_selection(&uids)?;
    spawn(
        "Purge",
        format!("purge:{folder_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            owned_folder(db, &acc, folder_id)?;
            let mut imap = checkout_session(&acc).await?;
            let n = imap
                .purge_uids(db, folder_id, &uids)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                format!("Deleted {n} permanently"),
                Some(JobRefresh::folder(acc.id, folder_id)),
            ))
        },
    )
}

/// Move a selection to the Archive folder, creating it when the account has
/// none — one-click archive should not first make the user set up a folder.
pub fn archive_messages(account_id: i64, folder_id: i64, uids: Vec<u32>) -> anyhow::Result<()> {
    require_selection(&uids)?;
    spawn(
        "Archive",
        format!("archive:{folder_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            let folder = owned_folder(db, &acc, folder_id)?;
            let mut imap = checkout_session(&acc).await?;
            let known = folders::list_by_account(db, acc.id).map_err(|e| e.to_string())?;
            let archive = match known.iter().find(|f| f.role == FolderRole::Archive) {
                Some(a) => a.clone(),
                None => {
                    let delim = known
                        .first()
                        .map(|f| f.delimiter.clone())
                        .unwrap_or_else(|| "/".to_string());
                    imap.create_folder_path(db, acc.id, "Archive", &delim)
                        .await
                        .map_err(|e| e.to_string())?
                }
            };
            let summary = if archive.id == folder.id {
                "Already in Archive".to_string()
            } else {
                let n = imap
                    .move_uids_to(db, folder_id, &uids, &archive.path)
                    .await
                    .map_err(|e| e.to_string())?;
                format!("Archived {n} to {}", archive.path)
            };
            imap.checkin();
            Ok((summary, Some(JobRefresh::folder(acc.id, folder_id))))
        },
    )
}

/// Move a selection to any folder of the same account, addressed by path so
/// subfolders come along for free.
pub fn move_messages(
    account_id: i64,
    folder_id: i64,
    uids: Vec<u32>,
    dest_path: String,
) -> anyhow::Result<()> {
    require_selection(&uids)?;
    spawn(
        "Move",
        format!("move:{folder_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            let folder = owned_folder(db, &acc, folder_id)?;
            let dest = folders::get_by_path(db, acc.id, &dest_path).map_err(|e| e.to_string())?;
            if dest.id == folder.id {
                return Ok(("Already here".to_string(), None));
            }
            let mut imap = checkout_session(&acc).await?;
            let n = imap
                .move_uids_to(db, folder_id, &uids, &dest.path)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                format!("Moved {n} to {}", dest.path),
                Some(JobRefresh::folder(acc.id, folder_id)),
            ))
        },
    )
}

/// Create an IMAP folder. `/` separates levels in `path` and is mapped onto
/// the account's own hierarchy delimiter; missing parents are created too and
/// an existing path is success, not an error.
pub fn create_folder(account_id: i64, path: String) -> anyhow::Result<()> {
    spawn(
        "Folders",
        format!("create-folder:{account_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            let delim = folders::list_by_account(db, acc.id)
                .unwrap_or_default()
                .first()
                .map(|f| f.delimiter.clone())
                .unwrap_or_else(|| "/".to_string());
            let mut imap = checkout_session(&acc).await?;
            let created = imap
                .create_folder_path(db, acc.id, &path, &delim)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                format!("Created {}", created.path),
                Some(JobRefresh::account(acc.id)),
            ))
        },
    )
}

fn require_selection(uids: &[u32]) -> anyhow::Result<()> {
    if uids.is_empty() {
        anyhow::bail!("no messages selected");
    }
    Ok(())
}

/// A folder id is worthless without the account it belongs to: ids are
/// per-database, and a UI that switched accounts mid-action would otherwise
/// delete mail out of the wrong mailbox.
fn owned_folder(
    db: &mailcore::Db,
    account: &mailcore::models::Account,
    folder_id: i64,
) -> Result<mailcore::models::Folder, String> {
    let folder = folders::get(db, folder_id).map_err(|e| e.to_string())?;
    if folder.account_id != account.id {
        return Err("folder does not belong to this account".to_string());
    }
    Ok(folder)
}
