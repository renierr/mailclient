//! Verbs that change the mailbox: flags, copy, move, expunge, APPEND and
//! CREATE.
//!
//! Destroying messages is the delicate one -- see [`ImapSession::uid_expunge`].

use std::collections::HashSet;

use imap_types::sequence::SequenceSet;

use super::super::types::vec1;
use super::super::utf7::mailbox_for_wire;
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
        let mailbox = mailbox_for_wire(dest)?;
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
        let mailbox = mailbox_for_wire(dest)?;

        if has_move {
            let body = CommandBody::Move {
                sequence_set: sequence_set.clone(),
                mailbox: mailbox.clone(),
                uid: true,
            };
            if let Err(e) = self.execute(body).await {
                if self.is_broken() {
                    return Err(e);
                }
                log::warn!("imap: UID MOVE failed ({e}), falling back to COPY + STORE + EXPUNGE");
                self.copy_then_remove(uids, sequence_set, mailbox).await?;
            }
        } else {
            self.copy_then_remove(uids, sequence_set, mailbox).await?;
        }
        Ok(())
    }

    /// MOVE for servers without it: COPY, then flag and expunge the source.
    ///
    /// Once the COPY landed the move has happened for the user, so a failing
    /// STORE or EXPUNGE afterwards is logged, not returned. Returning it made
    /// the caller retry the whole move, and every retry copied again: one more
    /// duplicate in the destination per attempt (B11). The worst case now is
    /// the source copy staying behind, which the next sync shows and the user
    /// can delete.
    async fn copy_then_remove(
        &mut self,
        uids: &[u32],
        sequence_set: SequenceSet,
        mailbox: Mailbox<'static>,
    ) -> Result<()> {
        let body = CommandBody::copy(sequence_set, mailbox, true)
            .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
        self.execute(body).await?;
        let removed = match self
            .uid_store_flags(uids, StoreType::Add, vec![Flag::Deleted])
            .await
        {
            Ok(()) => self.uid_expunge(uids).await,
            Err(e) => Err(e),
        };
        if let Err(e) = removed {
            log::warn!("imap: copied, but the source copies were not removed: {e}");
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
            return self.expunge_only(uids).await;
        }
        let sequence_set = uids_to_sequence_set(uids)?;
        match self.execute(CommandBody::ExpungeUid { sequence_set }).await {
            Ok(_) => Ok(()),
            Err(e) if self.is_broken() => Err(e),
            Err(e) => {
                // Advertised but refused: the messages are already flagged
                // `\Deleted`, so leaving them is the wrong outcome too.
                log::warn!("imap: UID EXPUNGE failed ({e}), falling back to EXPUNGE");
                self.expunge_only(uids).await
            }
        }
    }

    /// The mailbox-wide EXPUNGE, but only when it destroys nothing beyond
    /// `uids`. It removes every `\Deleted` message, including ones another
    /// client flagged and has not expunged yet (B11b), so it first asks the
    /// server which are flagged. Anything else flagged: refuse, and leave
    /// ours flagged for a later expunge.
    async fn expunge_only(&mut self, uids: &[u32]) -> Result<()> {
        let ours: HashSet<u32> = uids.iter().copied().collect();
        let others = self
            .uid_search(vec1![SearchKey::Deleted])
            .await?
            .into_iter()
            .filter(|u| !ours.contains(u))
            .count();
        if others > 0 {
            return Err(StoreError::Network(format!(
                "not expunged: {others} other message(s) in this folder are marked deleted, \
                 and the server cannot expunge by UID"
            )));
        }
        self.expunge().await
    }

    /// APPEND.
    pub async fn append(
        &mut self,
        folder: &str,
        raw: &[u8],
        flags: Vec<Flag<'static>>,
    ) -> Result<()> {
        let mailbox = mailbox_for_wire(folder)?;
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
        let mailbox = mailbox_for_wire(folder)?;
        let body = CommandBody::create(mailbox)
            .map_err(|e| StoreError::InvalidInput(format!("create args: {e}")))?;
        self.execute(body).await?;
        Ok(())
    }
}
