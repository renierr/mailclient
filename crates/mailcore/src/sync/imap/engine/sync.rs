//! Bringing the local cache in line with the server, in both directions.
//!
//! Down: a newest-N window, delta-synced through CONDSTORE / QRESYNC where
//! the server offers them, with older mail backfilled on demand. Up: the
//! flag changes the UI queued locally so a click never waited on IMAP.

use super::*;

impl ImapSync {
    /// Synchronize a folder window using CONDSTORE / QRESYNC delta sync when supported.
    pub async fn sync_folder_window(
        &mut self,
        db: &Db,
        folder_id: i64,
        window: Option<usize>,
    ) -> Result<SyncReport> {
        let folder = folders::get(db, folder_id)?;
        let account = accounts::get(db, folder.account_id)?;
        let session = self.session()?;

        let qresync_param = folder.uid_validity.map(|v| (v, folder.highest_modseq));
        let mb = session.select(&folder.path, qresync_param).await?;
        log::info!(
            "imap: SELECT {} ({} mails, uid_next {:?}, modseq {:?}, vanished: {})",
            folder.path,
            mb.exists,
            mb.uid_next,
            mb.highest_modseq,
            mb.vanished.len()
        );

        // UIDVALIDITY change => server-side rebuild, drop local copies.
        let validity_changed = mb
            .uid_validity
            .is_some_and(|v| folder.uid_validity.is_some_and(|old| old != v));
        if validity_changed {
            log::warn!("imap: UIDVALIDITY changed for {} — resyncing", folder.path);
            messages::delete_by_folder(db, folder_id)?;
        }

        let mut expunged = 0u64;

        // 1. Process QRESYNC VANISHED ranges immediately, with range
        // deletes so a `VANISHED 1:100000` never materializes 100k UIDs.
        if !mb.vanished.is_empty() {
            let vanished_count: u64 = mb
                .vanished
                .iter()
                .map(|(lo, hi)| u64::from(hi.saturating_sub(*lo).saturating_add(1)))
                .sum();
            log::info!(
                "imap: QRESYNC reported {} vanished range(s) ({} uids) in {}",
                mb.vanished.len(),
                vanished_count,
                folder.path
            );
            for (lo, hi) in &mb.vanished {
                expunged += messages::delete_by_uid_range(db, folder_id, *lo, *hi)?;
            }
        }

        let local_uids: HashSet<u32> = messages::list_uids(db, folder_id)?.into_iter().collect();
        let (server_uids, search_lo) = search_recent_uids(session, window, mb.uid_next).await?;

        // Newest-N relevance window: UIDs grow monotonically, so the largest
        // N are the newest. Everything outside costs no network.
        let relevant: Option<HashSet<u32>> = window.map(|n| {
            let mut sorted: Vec<u32> = server_uids.iter().copied().collect();
            sorted.sort_unstable();
            let skip = sorted.len().saturating_sub(n);
            sorted.into_iter().skip(skip).collect()
        });
        let relevant_len = relevant.as_ref().map(|r| r.len()).unwrap_or(0);
        let in_window = |uid: &u32| relevant.as_ref().is_none_or(|r| r.contains(uid));
        let is_trash = folder.role == FolderRole::Trash;

        // 2. Flag refresh for messages we already have within the window.
        // This guarantees that whatever messages are currently in view have 100%
        // accurate flags and unread counts matching the server.
        let existing: Vec<u32> = server_uids
            .intersection(&local_uids)
            .copied()
            .filter(in_window)
            .collect();
        for chunk in existing.chunks(FETCH_CHUNK) {
            let changed = session.uid_fetch_flags_changesince(chunk, 0).await?;
            for (uid, flags, _) in changed {
                let (read, starred, draft) = flag_state(&flags);
                let read = read || is_trash;
                messages::set_flags_by_uid(
                    db,
                    account.id,
                    folder_id,
                    uid,
                    read || draft,
                    starred,
                    draft,
                )?;
            }
        }

        // 3. If CONDSTORE is enabled, also check for flag changes on older local messages
        // that fall outside the active window using CHANGEDSINCE.
        if session.condstore_enabled && folder.highest_modseq > 0 {
            let older_existing: Vec<u32> = local_uids
                .iter()
                .copied()
                .filter(|u| !in_window(u))
                .collect();
            for chunk in older_existing.chunks(FETCH_CHUNK) {
                let changed = session
                    .uid_fetch_flags_changesince(chunk, folder.highest_modseq)
                    .await?;
                for (uid, flags, _) in changed {
                    let (read, starred, draft) = flag_state(&flags);
                    let read = read || is_trash;
                    messages::set_flags_by_uid(
                        db,
                        account.id,
                        folder_id,
                        uid,
                        read || draft,
                        starred,
                        draft,
                    )?;
                }
            }
        }

        // 4. Full fetch of new messages (windowed). BODY.PEEK[] is mandatory here.
        let mut fetched = 0u64;
        let mut missing: Vec<u32> = server_uids
            .difference(&local_uids)
            .copied()
            .filter(in_window)
            .collect();
        missing.sort_unstable();

        for chunk in missing.chunks(FETCH_CHUNK) {
            let messages_data = session.uid_fetch_messages(chunk).await?;
            for (uid, flags, raw) in messages_data {
                let (mut parsed, files) =
                    parse_to_new(account.id, folder_id, uid, &flags, &raw, false)?;
                if is_trash {
                    parsed.is_read = true;
                }
                let id = messages::upsert(db, &parsed)?;
                collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
                store_attachment_meta(db, id, files);
                fetched += 1;
            }
        }

        if let Some(n) = window {
            let skipped = (mb.exists as usize).saturating_sub(relevant_len);
            if skipped > 0 {
                log::info!(
                    "imap: {} skipped {} old mails outside window {n}",
                    folder.path,
                    skipped
                );
            }
        }

        // 5. Expunge locally what the searched UID range no longer has.
        // UIDs below `search_lo` were never asked about, so they stay cached.
        // This diffing runs locally at 0 network cost to catch server deletions.
        for uid in &local_uids {
            if *uid >= search_lo && !server_uids.contains(uid) {
                messages::delete_by_uid(db, folder_id, *uid)?;
                expunged += 1;
            }
        }

        // 6. If this is Trash, ensure any unread messages in local DB are marked \Seen on server.
        if is_trash {
            if let Ok(unread_uids) = messages::list_unread_uids(db, folder_id) {
                if !unread_uids.is_empty() {
                    if let Err(e) = session
                        .uid_store_flags(&unread_uids, StoreType::Add, vec![Flag::Seen])
                        .await
                    {
                        log::warn!("imap: trash seen sweep failed: {e}");
                    }
                    for uid in &unread_uids {
                        let _ = messages::set_flags_by_uid(
                            db, account.id, folder_id, *uid, true, false, false,
                        );
                    }
                }
            }
        }

        let validity = mb.uid_validity.unwrap_or(folder.uid_validity.unwrap_or(0));
        let uid_next = mb.uid_next.unwrap_or(folder.uid_next.unwrap_or(0));
        // Modseqs belong to a mailbox incarnation. Carrying the old one into
        // a rebuilt mailbox that reports none of its own would make the next
        // sync ask `CHANGEDSINCE <stale>` and QRESYNC-select against it —
        // both silently skipping everything older than a number from a
        // mailbox that no longer exists. Starting over is the only safe read.
        let new_modseq = if validity_changed {
            mb.highest_modseq.unwrap_or(0)
        } else {
            mb.highest_modseq.unwrap_or(folder.highest_modseq)
        };
        folders::set_sync_state(
            db,
            folder_id,
            validity,
            uid_next,
            u64::from(mb.exists),
            new_modseq,
        )?;

        Ok(SyncReport {
            fetched,
            expunged,
            folders: 0,
        })
    }

    pub async fn sync_older(
        &mut self,
        db: &Db,
        folder_id: i64,
        batch: usize,
    ) -> Result<SyncReport> {
        let folder = folders::get(db, folder_id)?;
        let account = accounts::get(db, folder.account_id)?;
        let session = self.session()?;

        let mb = session.select(&folder.path, None).await?;
        if let Some(validity) = mb.uid_validity {
            if folder.uid_validity.is_some_and(|v| v != validity) {
                return self
                    .sync_folder_window(db, folder_id, Some(FULL_SYNC_WINDOW))
                    .await;
            }
        }

        let min_uid = match messages::min_uid(db, folder_id)? {
            Some(u) => u,
            None => {
                return self
                    .sync_folder_window(db, folder_id, Some(FULL_SYNC_WINDOW))
                    .await;
            }
        };

        if min_uid <= 1 {
            return Ok(SyncReport::default());
        }

        let hi = min_uid.saturating_sub(1);
        let seq = SequenceSet::try_from(format!("1:{hi}").as_str())
            .map_err(|e| StoreError::InvalidInput(format!("seq: {e}")))?;
        let mut server_uids = session.uid_search(vec1![SearchKey::Uid(seq)]).await?;
        server_uids.sort_unstable();

        let local_uids: HashSet<u32> = messages::list_uids(db, folder_id)?.into_iter().collect();
        let missing: Vec<u32> = server_uids
            .into_iter()
            .rev()
            .filter(|u| !local_uids.contains(u))
            .take(batch)
            .collect();

        let mut fetched = 0u64;
        let mut missing_sorted = missing;
        missing_sorted.sort_unstable();

        for chunk in missing_sorted.chunks(FETCH_CHUNK) {
            let messages_data = session.uid_fetch_messages(chunk).await?;
            for (uid, flags, raw) in messages_data {
                let (parsed, files) =
                    parse_to_new(account.id, folder_id, uid, &flags, &raw, false)?;
                let id = messages::upsert(db, &parsed)?;
                collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
                store_attachment_meta(db, id, files);
                fetched += 1;
            }
        }

        let validity = mb.uid_validity.unwrap_or(folder.uid_validity.unwrap_or(0));
        let uid_next = mb.uid_next.unwrap_or(folder.uid_next.unwrap_or(0));
        let modseq = mb.highest_modseq.unwrap_or(folder.highest_modseq);
        folders::set_sync_state(
            db,
            folder_id,
            validity,
            uid_next,
            u64::from(mb.exists),
            modseq,
        )?;

        Ok(SyncReport {
            fetched,
            expunged: 0,
            folders: 0,
        })
    }

    /// Download attachments for one message (attachment-download click).
    /// Re-fetches the full body with PEEK (never implicitly marks `\Seen`),
    /// stores every part with bytes, and refreshes only the
    /// `has_attachments` flag — read/star state is never touched. Returns
    /// the number of stored files.
    pub async fn fetch_attachments(&mut self, db: &Db, message_id: i64) -> Result<u64> {
        let msg = messages::get(db, message_id)?;
        let folder = folders::get(db, msg.folder_id)?;
        let session = self.session()?;
        session.select(&folder.path, None).await?;

        let fetched = session.uid_fetch_messages(&[msg.uid]).await?;
        let raw = match fetched.into_iter().next() {
            Some((_, _, r)) => r,
            None => {
                return Err(StoreError::InvalidInput(format!(
                    "message uid {} no longer on server",
                    msg.uid
                )));
            }
        };

        let parsed = mail_parser::MessageParser::default()
            .parse(&raw)
            .ok_or_else(|| StoreError::InvalidInput("parse failed".to_string()))?;
        let files = extract_attachments(&parsed, true);
        let stored = files.len() as u64;
        store_attachments(db, message_id, files)?;
        messages::set_has_attachments(db, message_id, stored > 0)?;
        log::info!("imap: downloaded {stored} attachment(s) for message {message_id}");
        Ok(stored)
    }

    /// Push every locally-dirtied flag change, clearing each row on success.
    /// Rows that fail stay dirty for the next run, so this never loses a
    /// toggle (offline, quit mid-push, server error). Returns pushed count.
    /// Used both by full syncs and by the quiet post-toggle push job.
    ///
    /// A row the user toggled again mid-push also stays dirty — the clear
    /// only matches the flags this run actually sent (see
    /// [`messages::clear_flags_dirty`]).
    pub async fn push_dirty_flags(&mut self, db: &Db, account_id: i64) -> u64 {
        let mut pushed = 0u64;
        for m in messages::list_flags_dirty(db, account_id).unwrap_or_default() {
            if self.push_flags(db, &m).await.is_ok() {
                match messages::clear_flags_dirty(db, m.id, m.is_read, m.is_starred) {
                    Ok(false) => log::info!(
                        "imap: message {} was toggled again mid-push, staying dirty",
                        m.id
                    ),
                    Ok(true) => {}
                    Err(e) => log::warn!("imap: cannot clear dirty flag on {}: {e}", m.id),
                }
                pushed += 1;
            }
        }
        pushed
    }
}
