//! What the user's actions mean on the server: delete, archive, move,
//! file a copy, create a folder.
//!
//! The policy of what "delete" means (Trash, or destroy for spam and mail
//! already in Trash) lives in [`crate::undo`]; this is the wire side.

use super::*;

/// Failed pushes before a pending move is dropped (see
/// [`ImapSync::push_due_moves`]).
pub const MAX_PENDING_ATTEMPTS: i64 = 5;

/// Pending moves that go out as one `UID MOVE`: source folder, action, target.
type MoveGroup = (i64, crate::store::pending_moves::PendingAction, Option<i64>);
/// One queued message: `(message_id, uid, attempts)`.
type PendingItem = (i64, u32, i64);

impl ImapSync {
    pub async fn append_to_folder(&mut self, folder_path: &str, raw: &[u8]) -> Result<()> {
        let session = self.session()?;
        session.append(folder_path, raw, vec![Flag::Seen]).await
    }

    pub async fn append_draft(&mut self, folder_path: &str, raw: &[u8]) -> Result<()> {
        let session = self.session()?;
        // Drafts are deliberately not marked seen: the `\Draft` flag is what
        // makes providers keep them out of normal send flows.
        session.append(folder_path, raw, vec![Flag::Draft]).await
    }

    /// Create an IMAP mailbox (plus any missing parents) and register it
    /// locally via folder discovery. Returns the created [`Folder`].
    /// An already-existing path is success, not an error — discovery simply
    /// returns it.
    pub async fn create_folder_path(
        &mut self,
        db: &Db,
        account_id: i64,
        path: &str,
        delimiter: &str,
    ) -> Result<Folder> {
        let normalized = normalize_folder_path(path, delimiter)?;
        let mut prefix = String::new();
        for segment in normalized.split(delimiter) {
            if !prefix.is_empty() {
                prefix.push_str(delimiter);
            }
            prefix.push_str(segment);
            match self.session()?.create_folder(&prefix).await {
                Ok(()) => log::info!("imap: created folder {prefix}"),
                Err(e) if is_already_exists(&e) => {
                    log::debug!("imap: folder exists: {prefix}")
                }
                Err(e) => return Err(e),
            }
        }
        self.sync_folders(db, account_id).await?;
        folders::get_by_path(db, account_id, &normalized)
            .map_err(|_| StoreError::InvalidInput(format!("server did not list {normalized}")))
    }

    pub async fn delete_message(&mut self, db: &Db, message_id: i64) -> Result<()> {
        let msg = messages::get(db, message_id)?;
        let folder = folders::get(db, msg.folder_id)?;
        let session = self.session()?;
        session.select(&folder.path, None).await?;
        session
            .uid_store_flags(&[msg.uid], StoreType::Add, vec![Flag::Deleted])
            .await?;
        session.uid_expunge(&[msg.uid]).await?;
        messages::delete(db, message_id)?;
        Ok(())
    }

    pub async fn move_uids_to(
        &mut self,
        db: &Db,
        src_folder_id: i64,
        uids: &[u32],
        dest_path: &str,
    ) -> Result<u64> {
        let mut clean: Vec<u32> = uids.to_vec();
        clean.sort_unstable();
        clean.dedup();
        if clean.is_empty() {
            return Ok(0);
        }
        let src = folders::get(db, src_folder_id)?;
        if src.path == dest_path {
            return Ok(0);
        }
        let trash = folders::list_by_account(db, src.account_id)?
            .into_iter()
            .find(|f| f.role == FolderRole::Trash);
        let dest_is_trash = trash.as_ref().is_some_and(|t| t.path == dest_path);

        let session = self.session()?;
        session.select(&src.path, None).await?;
        if dest_is_trash {
            if let Err(e) = session
                .uid_store_flags(&clean, StoreType::Add, vec![Flag::Seen])
                .await
            {
                log::warn!("imap: mark-seen before bulk trash move failed: {e}");
            }
        }
        session.uid_move(&clean, dest_path).await?;
        let count = messages::delete_many_by_uids(db, src_folder_id, &clean)?;
        Ok(count)
    }

    pub async fn purge_uids(&mut self, db: &Db, folder_id: i64, uids: &[u32]) -> Result<u64> {
        if uids.is_empty() {
            return Ok(0);
        }
        let folder = folders::get(db, folder_id)?;
        let session = self.session()?;
        session.select(&folder.path, None).await?;
        session
            .uid_store_flags(uids, StoreType::Add, vec![Flag::Deleted])
            .await?;
        session.uid_expunge(uids).await?;
        let count = messages::delete_many_by_uids(db, folder_id, uids)?;
        Ok(count)
    }

    /// Push every pending move of `account_id` whose grace period is over
    /// (see [`crate::store::pending_moves`]). One `UID MOVE` per source
    /// folder and target, so a bulk delete stays one round trip.
    ///
    /// A failed group stays pending and is retried by the next sync; after
    /// [`MAX_PENDING_ATTEMPTS`] it is dropped and the messages show again
    /// where they were, rather than staying hidden forever. A target that no
    /// longer exists (Trash gone) drops the group the same way — nothing is
    /// ever destroyed here that the user did not confirm. Returns moved count.
    pub async fn push_due_moves(&mut self, db: &Db, account_id: i64) -> u64 {
        use crate::store::pending_moves;
        let due = match pending_moves::list_due(db, account_id, &crate::store::now()) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("imap: cannot read pending moves: {e}");
                return 0;
            }
        };
        let mut groups: Vec<(MoveGroup, Vec<PendingItem>)> = Vec::new();
        for p in due {
            let key = (p.folder_id, p.action, p.dest_folder_id);
            let item = (p.message_id, p.uid, p.attempts);
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, items)) => items.push(item),
                None => groups.push((key, vec![item])),
            }
        }
        let mut moved = 0u64;
        for ((src_id, action, dest_id), items) in groups {
            let ids: Vec<i64> = items.iter().map(|(id, _, _)| *id).collect();
            let uids: Vec<u32> = items.iter().map(|(_, uid, _)| *uid).collect();
            let dest = match self
                .pending_target(db, account_id, src_id, action, dest_id)
                .await
            {
                Ok(Some(path)) => path,
                Ok(None) => {
                    log::warn!(
                        "imap: pending {} has no target any more, dropped",
                        action.as_str()
                    );
                    let _ = pending_moves::remove(db, &ids);
                    continue;
                }
                Err(e) => {
                    log::warn!("imap: pending {} target failed: {e}", action.as_str());
                    self.pending_failed(db, &items);
                    continue;
                }
            };
            match self.move_uids_to(db, src_id, &uids, &dest).await {
                Ok(n) => moved += n,
                Err(e) => {
                    log::warn!("imap: pending {} to {dest} failed: {e}", action.as_str());
                    self.pending_failed(db, &items);
                }
            }
        }
        moved
    }

    async fn pending_target(
        &mut self,
        db: &Db,
        account_id: i64,
        src_id: i64,
        action: crate::store::pending_moves::PendingAction,
        dest_id: Option<i64>,
    ) -> Result<Option<String>> {
        use crate::store::pending_moves::PendingAction;
        let known = folders::list_by_account(db, account_id)?;
        let by_role = |role: FolderRole| {
            known
                .iter()
                .find(|f| f.role == role && f.id != src_id)
                .map(|f| f.path.clone())
        };
        Ok(match action {
            PendingAction::Trash => by_role(FolderRole::Trash),
            PendingAction::Move => dest_id
                .and_then(|id| known.iter().find(|f| f.id == id && f.id != src_id))
                .map(|f| f.path.clone()),
            PendingAction::Archive => match dest_id {
                Some(id) => known.iter().find(|f| f.id == id).map(|f| f.path.clone()),
                None => match by_role(FolderRole::Archive) {
                    Some(p) => Some(p),
                    None => {
                        let delim = known
                            .first()
                            .map(|f| f.delimiter.clone())
                            .unwrap_or_else(|| "/".to_string());
                        let created = self
                            .create_folder_path(db, account_id, "Archive", &delim)
                            .await?;
                        (created.id != src_id).then_some(created.path)
                    }
                },
            },
        })
    }

    fn pending_failed(&self, db: &Db, items: &[PendingItem]) {
        use crate::store::pending_moves;
        let (give_up, retry): (Vec<_>, Vec<_>) = items
            .iter()
            .partition(|(_, _, attempts)| attempts + 1 >= MAX_PENDING_ATTEMPTS);
        let give_up: Vec<i64> = give_up.iter().map(|(id, _, _)| *id).collect();
        let retry: Vec<i64> = retry.iter().map(|(id, _, _)| *id).collect();
        if !give_up.is_empty() {
            log::warn!("imap: giving up on {} pending move(s)", give_up.len());
            let _ = pending_moves::remove(db, &give_up);
        }
        let _ = pending_moves::record_failure(db, &retry);
    }
}
