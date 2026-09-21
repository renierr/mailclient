//! What the user's actions mean on the server: delete, archive, move,
//! file a copy, create a folder.
//!
//! "Delete" is the one with real policy behind it -- spam and a message
//! already in Trash are destroyed, everything else is moved (see
//! [`ImapSync::trash_message`]).

use super::*;

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

    /// Move one message to the account's Trash folder -- what "delete" means
    /// in a mail client, with two exceptions that destroy immediately:
    ///
    /// - the message is already in Trash (deleting from Trash is permanent),
    /// - the message is spam (filing junk into Trash just moves garbage
    ///   around — it is destroyed instead).
    pub async fn trash_message(&mut self, db: &Db, message_id: i64) -> Result<TrashOutcome> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;

        // Spam never touches Trash; Trash never keeps a second copy of itself.
        if folder.role == FolderRole::Junk {
            log::info!("imap: destroying spam directly (uid {})", message.uid);
            self.delete_message(db, message_id).await?;
            return Ok(TrashOutcome::Expunged);
        }
        let trash = folders::list_by_account(db, message.account_id)?
            .into_iter()
            .find(|f| f.role == FolderRole::Trash);

        // Already in Trash, or no Trash at all: the only remaining meaning of
        // "delete" is destroying it, and the caller is told so.
        let Some(trash) = trash.filter(|t| t.id != folder.id) else {
            self.delete_message(db, message_id).await?;
            return Ok(TrashOutcome::Expunged);
        };

        let session = self.session()?;
        session.select(&folder.path, None).await?;
        if let Err(e) = session
            .uid_store_flags(&[message.uid], StoreType::Add, vec![Flag::Seen])
            .await
        {
            log::warn!("imap: mark-seen before trash move failed: {e}");
        }
        session.uid_move(&[message.uid], &trash.path).await?;
        messages::delete(db, message_id)?;
        Ok(TrashOutcome::Moved(trash.path))
    }

    pub async fn archive_message(&mut self, db: &Db, message_id: i64) -> Result<ArchiveOutcome> {
        let message = messages::get(db, message_id)?;
        let folder = folders::get(db, message.folder_id)?;
        let archive = folders::list_by_account(db, message.account_id)?
            .into_iter()
            .find(|f| f.role == FolderRole::Archive);

        let archive = match archive {
            Some(a) => a,
            None => {
                let delim = folders::list_by_account(db, message.account_id)?
                    .first()
                    .map(|f| f.delimiter.clone())
                    .unwrap_or_else(|| "/".to_string());
                self.create_folder_path(db, message.account_id, "Archive", &delim)
                    .await?
            }
        };

        if archive.id == folder.id {
            return Ok(ArchiveOutcome::AlreadyThere);
        }

        let session = self.session()?;
        session.select(&folder.path, None).await?;
        session.uid_move(&[message.uid], &archive.path).await?;
        messages::delete(db, message_id)?;
        Ok(ArchiveOutcome::Moved(archive.path))
    }

    pub async fn move_to_folder(
        &mut self,
        db: &Db,
        message_id: i64,
        dest_folder_id: i64,
    ) -> Result<MoveOutcome> {
        let msg = messages::get(db, message_id)?;
        let src_folder = folders::get(db, msg.folder_id)?;
        let dest_folder = folders::get(db, dest_folder_id)?;
        if dest_folder.account_id != msg.account_id {
            return Err(StoreError::InvalidInput(
                "destination folder belongs to another account".to_string(),
            ));
        }
        if msg.folder_id == dest_folder_id {
            return Ok(MoveOutcome::AlreadyThere);
        }

        let session = self.session()?;
        session.select(&src_folder.path, None).await?;
        if dest_folder.role == FolderRole::Trash {
            if let Err(e) = session
                .uid_store_flags(&[msg.uid], StoreType::Add, vec![Flag::Seen])
                .await
            {
                log::warn!("imap: mark-seen before move to trash failed: {e}");
            }
        }
        session.uid_move(&[msg.uid], &dest_folder.path).await?;
        messages::delete(db, message_id)?;
        Ok(MoveOutcome::Moved(dest_folder.path))
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
}
