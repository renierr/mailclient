//! Server-side message moves and deletes.
//!
//! Delete, archive and move are undoable: `mailcore::undo` hides the
//! messages at once and the IMAP move runs after the grace period, so these
//! return an Undo handle instead of starting a job. Only a delete that
//! destroys (and an explicit purge) is a job.
//!
//! All of these take a *selection* — a list of UIDs — rather than having a
//! single-message and a bulk variant of each. The Qt bridge grew both, and
//! they drifted; a one-element list is the same operation, and the IMAP
//! commands underneath (`UID MOVE`, `UID STORE`+`UID EXPUNGE`) are set-shaped
//! anyway, so a hundred selected mails cost one round trip, not a hundred.

use mailcore::bulk;
use mailcore::store::folders;
use mailcore::undo::{self, MoveTarget, Queued};

use crate::db::shared_db;
use crate::net::{spawn, spawn_push_after_grace, JobRefresh};
use mailcore::sync::pool::{checkout_session, resolve_account};

/// Result of an undoable action.
///
/// `batch` is the Undo handle for [`undo_move`]; empty means there is
/// nothing to undo — either a permanent delete job started (`purging`, its
/// result arrives as a `Purge` job event) or nothing happened (`label` says
/// why).
pub struct MoveResult {
    pub batch: String,
    pub label: String,
    pub purging: bool,
}

/// Delete a selection — which means Trash, undoable, except where it cannot.
///
/// Junk is destroyed outright (spam never passes through Trash), and so is
/// anything deleted from inside Trash itself or in an account that has no
/// Trash folder at all: that starts a purge job instead, because the
/// difference is not recoverable. The UI confirms that case first.
pub fn delete_messages(
    account_id: i64,
    folder_id: i64,
    uids: Vec<u32>,
) -> anyhow::Result<MoveResult> {
    queue(account_id, folder_id, uids, MoveTarget::Trash)
}

/// Destroy a selection server-side (`\Deleted` + expunge). No undo — only for
/// an explicit "delete permanently".
pub fn purge_messages(account_id: i64, folder_id: i64, uids: Vec<u32>) -> anyhow::Result<()> {
    require_selection(&uids)?;
    purge_groups(
        account_id,
        format!("purge:{folder_id}"),
        vec![(folder_id, uids)],
        Some(folder_id),
    )
}

/// One search hit: the folder it lives in and its UID there.
pub struct Hit {
    pub folder: String,
    pub uid: u32,
}

/// Search hits of one account grouped by folder (`mailcore::bulk`).
pub(crate) fn groups(
    db: &mailcore::Db,
    account_id: i64,
    hits: Vec<Hit>,
) -> anyhow::Result<bulk::Groups> {
    if hits.is_empty() {
        anyhow::bail!("no messages selected");
    }
    let hits: Vec<(String, u32)> = hits.into_iter().map(|h| (h.folder, h.uid)).collect();
    bulk::resolve_hits(db, account_id, &hits).map_err(anyhow::Error::msg)
}

/// Delete search hits across folders: one Undo for what goes to Trash, one
/// purge job for the shares that destroy (the UI confirmed those first).
pub fn delete_hits(account_id: i64, hits: Vec<Hit>) -> anyhow::Result<MoveResult> {
    queue_hits(account_id, hits, MoveTarget::Trash)
}

/// Archive search hits across folders, one Undo.
pub fn archive_hits(account_id: i64, hits: Vec<Hit>) -> anyhow::Result<MoveResult> {
    queue_hits(account_id, hits, MoveTarget::Archive)
}

/// Move search hits across folders to `dest_path`, one Undo.
pub fn move_hits(account_id: i64, hits: Vec<Hit>, dest_path: String) -> anyhow::Result<MoveResult> {
    queue_hits(account_id, hits, MoveTarget::Folder(dest_path))
}

/// Destroy search hits across folders in one job, over one session.
pub fn purge_hits(account_id: i64, hits: Vec<Hit>) -> anyhow::Result<()> {
    let groups = groups(shared_db()?, account_id, hits)?;
    purge_groups(account_id, format!("purge-hits:{account_id}"), groups, None)
}

fn queue_hits(account_id: i64, hits: Vec<Hit>, target: MoveTarget) -> anyhow::Result<MoveResult> {
    let db = shared_db()?;
    let groups = groups(db, account_id, hits)?;
    let moved = bulk::queue_move(db, account_id, &groups, target).map_err(anyhow::Error::msg)?;
    if moved.batch.is_some() {
        spawn_push_after_grace(account_id);
    }
    let purging = !moved.permanent.is_empty();
    if purging {
        purge_groups(
            account_id,
            format!("purge-hits:{account_id}"),
            moved.permanent,
            None,
        )?;
    }
    Ok(MoveResult {
        batch: moved.batch.unwrap_or_default(),
        label: moved.label,
        purging,
    })
}

/// The `Purge` job over `groups` (`mailcore::bulk::purge`). `folder` is the
/// one to refresh, or the whole account when the groups span folders.
fn purge_groups(
    account_id: i64,
    key: String,
    groups: bulk::Groups,
    folder: Option<i64>,
) -> anyhow::Result<()> {
    spawn("Purge", key, move |db, _progress| async move {
        let acc = resolve_account(db, account_id)?;
        let purged = bulk::purge(db, &acc, &groups).await?;
        let refresh = match folder {
            Some(f) => JobRefresh::folder(acc.id, f),
            None => JobRefresh::account(acc.id),
        };
        Ok((purged.status(), Some(refresh)))
    })
}

/// Move a selection to the Archive folder, undoable. The folder is created
/// on push when the account has none — one-click archive should not first
/// make the user set up a folder.
pub fn archive_messages(
    account_id: i64,
    folder_id: i64,
    uids: Vec<u32>,
) -> anyhow::Result<MoveResult> {
    queue(account_id, folder_id, uids, MoveTarget::Archive)
}

/// Move a selection to any folder of the same account, addressed by path so
/// subfolders come along for free. Undoable.
pub fn move_messages(
    account_id: i64,
    folder_id: i64,
    uids: Vec<u32>,
    dest_path: String,
) -> anyhow::Result<MoveResult> {
    queue(account_id, folder_id, uids, MoveTarget::Folder(dest_path))
}

/// Take back a queued action before it reaches the server. Returns the
/// status line text, also when it was too late.
pub fn undo_move(batch: String) -> anyhow::Result<String> {
    let n = undo::undo(shared_db()?, &batch).map_err(anyhow::Error::msg)?;
    Ok(match n {
        0 => "Too late to undo — already done on the server".to_string(),
        1 => "Undone: 1 message is back".to_string(),
        n => format!("Undone: {n} messages are back"),
    })
}

/// Seconds an action stays undoable.
#[flutter_rust_bridge::frb(sync)]
pub fn undo_grace_secs() -> i32 {
    undo::UNDO_GRACE_SECS as i32
}

fn queue(
    account_id: i64,
    folder_id: i64,
    uids: Vec<u32>,
    target: MoveTarget,
) -> anyhow::Result<MoveResult> {
    require_selection(&uids)?;
    let db = shared_db()?;
    let queued =
        undo::queue_move(db, account_id, folder_id, &uids, target).map_err(anyhow::Error::msg)?;
    Ok(match queued {
        Queued::Pending { batch, label, .. } => {
            spawn_push_after_grace(account_id);
            MoveResult {
                batch,
                label,
                purging: false,
            }
        }
        Queued::Permanent => {
            purge_messages(account_id, folder_id, uids)?;
            MoveResult {
                batch: String::new(),
                label: String::new(),
                purging: true,
            }
        }
        Queued::AlreadyThere => MoveResult {
            batch: String::new(),
            label: "Already here".to_string(),
            purging: false,
        },
    })
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
