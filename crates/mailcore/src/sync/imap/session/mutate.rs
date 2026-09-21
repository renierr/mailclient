//! Verbs that change the mailbox: flags, copy, move, expunge, APPEND and
//! CREATE.
//!
//! Destroying messages is the delicate one -- see [`ImapSession::uid_expunge`].

use super::*;

impl ImapSession {
    /// STORE flags (+FLAGS / -FLAGS).
    pub async fn uid_store_flags(
        &mut self,
        uids: &[u32],
        op: StoreType,
        flags: Vec<Flag<'static>>,
    ) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let sequence_set = uids_to_sequence_set(uids)?;

        let body = CommandBody::store(sequence_set, op, StoreResponse::Silent, flags, true)
            .map_err(|e| StoreError::InvalidInput(format!("store args: {e}")))?;

        self.execute(body).await?;
        Ok(())
    }

    /// UID COPY.
    pub async fn uid_copy(&mut self, uids: &[u32], dest: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let sequence_set = uids_to_sequence_set(uids)?;
        let mailbox = Mailbox::try_from(dest.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {dest}: {e}")))?;
        let body = CommandBody::copy(sequence_set, mailbox, true)
            .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
        self.execute(body).await?;
        Ok(())
    }

    /// UID MOVE (or fallback copy + deleted + expunge).
    pub async fn uid_move(&mut self, uids: &[u32], dest: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let has_move = self
            .capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case("move"));

        let sequence_set = uids_to_sequence_set(uids)?;
        let mailbox = Mailbox::try_from(dest.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {dest}: {e}")))?;

        if has_move {
            let body = CommandBody::Move {
                sequence_set: sequence_set.clone(),
                mailbox: mailbox.clone(),
                uid: true,
            };
            if let Err(e) = self.execute(body).await {
                log::warn!("imap: UID MOVE failed ({e}), falling back to COPY + STORE + EXPUNGE");
                let body = CommandBody::copy(sequence_set, mailbox, true)
                    .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
                self.execute(body).await?;
                self.uid_store_flags(uids, StoreType::Add, vec![Flag::Deleted])
                    .await?;
                self.uid_expunge(uids).await?;
            }
        } else {
            let body = CommandBody::copy(sequence_set, mailbox, true)
                .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
            self.execute(body).await?;
            self.uid_store_flags(uids, StoreType::Add, vec![Flag::Deleted])
                .await?;
            self.uid_expunge(uids).await?;
        }
        Ok(())
    }

    /// EXPUNGE — removes **every** `\Deleted` message in the mailbox.
    ///
    /// Only correct when that is genuinely what is meant. To destroy
    /// specific messages use [`Self::uid_expunge`], which does not reach
    /// past them.
    pub async fn expunge(&mut self) -> Result<()> {
        self.execute(CommandBody::Expunge).await?;
        Ok(())
    }

    /// UID EXPUNGE (RFC 4315): destroy only `uids`, among those flagged
    /// `\Deleted`.
    ///
    /// Plain `EXPUNGE` takes the whole mailbox with it, so a message another
    /// client had flagged `\Deleted` but not yet expunged — its own pending
    /// delete, still undoable on its side — was destroyed as a side effect of
    /// us deleting something unrelated. Servers without UIDPLUS leave no
    /// alternative, and there the wide behaviour is what "delete" has to
    /// mean; everywhere else this stays inside the selection the user made.
    pub async fn uid_expunge(&mut self, uids: &[u32]) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        if !self.has_capability("uidplus") {
            log::debug!("imap: no UIDPLUS, falling back to mailbox-wide EXPUNGE");
            return self.expunge().await;
        }
        let sequence_set = uids_to_sequence_set(uids)?;
        match self.execute(CommandBody::ExpungeUid { sequence_set }).await {
            Ok(_) => Ok(()),
            Err(e) => {
                // Advertised but refused: the messages are already flagged
                // `\Deleted`, so leaving them is the wrong outcome too.
                log::warn!("imap: UID EXPUNGE failed ({e}), falling back to EXPUNGE");
                self.expunge().await
            }
        }
    }

    /// APPEND.
    pub async fn append(
        &mut self,
        folder: &str,
        raw: &[u8],
        flags: Vec<Flag<'static>>,
    ) -> Result<()> {
        let mailbox = Mailbox::try_from(folder.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {folder}: {e}")))?;
        let literal = Literal::try_from(raw.to_vec())
            .map_err(|e| StoreError::InvalidInput(format!("literal error: {e}")))?;

        let body = CommandBody::Append {
            mailbox,
            flags,
            date: None,
            message: LiteralOrLiteral8::Literal(literal),
        };
        self.execute(body).await?;
        Ok(())
    }

    /// CREATE mailbox.
    pub async fn create_folder(&mut self, folder: &str) -> Result<()> {
        let mailbox = Mailbox::try_from(folder.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {folder}: {e}")))?;
        let body = CommandBody::create(mailbox)
            .map_err(|e| StoreError::InvalidInput(format!("create args: {e}")))?;
        self.execute(body).await?;
        Ok(())
    }
}
