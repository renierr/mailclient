//! [`ImapSync`]: high-level sync orchestration over [`super::session`].
//!
//! Owns the connection lifecycle (connect / LOGOUT / reconnect / NOOP health),
//! server search backfill, trash / archive / move, folder creation, windowed
//! sync, and attachment download. Folder discovery lives in [`super::folders`].

use std::collections::HashSet;

use imap_next::{
    client::{Client, Event, Options},
    stream::Stream,
};
use imap_types::{
    command::{Command, CommandBody},
    core::{AString, Tag},
    flag::{Flag, StoreType},
    response::{Status, StatusKind},
    search::SearchKey,
    sequence::SequenceSet,
};
use tokio::net::TcpStream;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Folder, FolderRole, Message};
use crate::store::{accounts, folders, messages};
use crate::sync::traits::{SyncProvider, SyncReport};

use super::{
    folders::discover_folders,
    parse::{
        collect_contacts_from_headers, extract_attachments, flag_state, parse_to_new,
        store_attachment_meta, store_attachments,
    },
    roles::{is_already_exists, normalize_folder_path},
    search::search_recent_uids,
    session::ImapSession,
    tls::{build_tls_connector, server_name_for},
    types::{
        endpoint_for, ArchiveOutcome, ImapEndpoint, MoveOutcome, ServerSearchReport, TrashOutcome,
        COMMAND_TIMEOUT, CONNECT_TIMEOUT, FETCH_CHUNK, FULL_SYNC_WINDOW,
    },
    vec1,
};

/// High-level IMAP synchronization engine.
pub struct ImapSync {
    endpoint: ImapEndpoint,
    account: crate::models::Account,
    pub(crate) session: Option<ImapSession>,
}

impl ImapSync {
    pub fn new(account: &crate::models::Account) -> Self {
        Self {
            endpoint: endpoint_for(account),
            account: account.clone(),
            session: None,
        }
    }

    /// Connect and authenticate with the server.
    pub async fn connect(&mut self, password: &str) -> Result<()> {
        if self.session.is_some() {
            return Ok(());
        }
        if !self.endpoint.implicit_tls && !self.endpoint.starttls {
            let sec = self.account.imap_security.trim().to_ascii_lowercase();
            if sec != "plain" && sec != "none" {
                return Err(StoreError::InvalidInput(format!(
                    "refusing plaintext IMAP connection to {}: set security to 'tls' or 'starttls'",
                    self.endpoint.addr
                )));
            }
        }

        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&self.endpoint.addr))
            .await
            .map_err(|_| {
                StoreError::Network(format!(
                    "connect timed out after {:?}: {}",
                    CONNECT_TIMEOUT, self.endpoint.addr
                ))
            })?
            .map_err(|e| StoreError::Network(format!("connect {}: {e}", self.endpoint.addr)))?;

        let (stream, client) = if self.endpoint.implicit_tls {
            let tls_connector = build_tls_connector()?;
            let server_name = server_name_for(&self.endpoint.host)?;
            let tls =
                tokio::time::timeout(CONNECT_TIMEOUT, tls_connector.connect(server_name, tcp))
                    .await
                    .map_err(|_| {
                        StoreError::Network(format!(
                            "TLS handshake timed out after {:?} with {}",
                            CONNECT_TIMEOUT, self.endpoint.addr
                        ))
                    })?
                    .map_err(|e| {
                        StoreError::Network(format!(
                            "TLS handshake failed with {}: {e}",
                            self.endpoint.addr
                        ))
                    })?;
            let mut stream = Stream::tls(tokio_rustls::TlsStream::Client(tls));
            let mut client = Client::new(Options::default());
            ImapSession::read_greeting(&mut stream, &mut client).await?;
            (stream, client)
        } else {
            let mut stream = Stream::insecure(tcp);
            let mut client = Client::new(Options::default());
            ImapSession::read_greeting(&mut stream, &mut client).await?;

            if self.endpoint.starttls {
                let tag = Tag::try_from("A0001")
                    .map_err(|e| StoreError::InvalidInput(format!("starttls tag invalid: {e}")))?;
                let handle = client.enqueue_command(
                    Command::new(tag.clone(), CommandBody::StartTLS).map_err(|e| {
                        StoreError::InvalidInput(format!("starttls command invalid: {e}"))
                    })?,
                );
                loop {
                    let event = tokio::time::timeout(COMMAND_TIMEOUT, stream.next(&mut client))
                        .await
                        .map_err(|_| {
                            StoreError::Network("timed out waiting for STARTTLS reply".to_string())
                        })?
                        .map_err(|e| StoreError::Network(format!("STARTTLS stream error: {e}")))?;
                    match event {
                        Event::StatusReceived {
                            status: Status::Tagged(tagged),
                        } => {
                            if tagged.tag == tag {
                                if tagged.body.kind == StatusKind::Ok {
                                    break;
                                } else {
                                    return Err(StoreError::Network(format!(
                                        "STARTTLS rejected: {}",
                                        tagged.body.text
                                    )));
                                }
                            } else {
                                log::debug!("imap: ignoring foreign tagged status: {tagged:?}");
                            }
                        }
                        Event::StatusReceived {
                            status: Status::Bye(bye),
                        } => {
                            return Err(StoreError::Network(format!(
                                "server sent BYE during STARTTLS: {}",
                                bye.text
                            )));
                        }
                        Event::CommandRejected {
                            handle: h, status, ..
                        } if h == handle => {
                            return Err(StoreError::Network(format!(
                                "STARTTLS rejected: {status:?}"
                            )));
                        }
                        _ => {}
                    }
                }

                let tcp_stream: TcpStream = stream.into();
                let tls_connector = build_tls_connector()?;
                let server_name = server_name_for(&self.endpoint.host)?;
                let tls = tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    tls_connector.connect(server_name, tcp_stream),
                )
                .await
                .map_err(|_| {
                    StoreError::Network(format!(
                        "STARTTLS handshake timed out after {:?} with {}",
                        CONNECT_TIMEOUT, self.endpoint.addr
                    ))
                })?
                .map_err(|e| {
                    StoreError::Network(format!(
                        "STARTTLS handshake failed with {}: {e}",
                        self.endpoint.addr
                    ))
                })?;
                let stream = Stream::tls(tokio_rustls::TlsStream::Client(tls));
                (stream, client)
            } else {
                (stream, client)
            }
        };

        let mut session = ImapSession::new(stream, client);

        let username = if self.account.imap_username.is_empty() {
            &self.account.email_address
        } else {
            &self.account.imap_username
        };

        session.login(username, password).await?;
        session.capability().await?;
        let _ = session.enable_extensions().await;

        self.session = Some(session);
        log::info!(
            "imap: connected and authenticated for {}",
            self.account.email_address
        );
        Ok(())
    }

    /// Best-effort async LOGOUT (sends `LOGOUT`, waits briefly for `BYE`,
    /// then drops the stream either way). Prefer this on explicit teardown;
    /// [`Self::disconnect`] is the non-blocking drop used by `Drop` paths
    /// where awaiting is impossible (a stale pooled connection would block
    /// quit on the `BYE` wait — closing the socket reaps server state just
    /// as well, like a network drop).
    pub async fn logout(&mut self) {
        if let Some(session) = self.session.as_mut() {
            let _ =
                tokio::time::timeout(COMMAND_TIMEOUT, session.execute(CommandBody::Logout)).await;
        }
        self.session = None;
    }

    pub fn disconnect(&mut self) {
        self.session = None;
    }

    pub async fn reconnect(&mut self, password: Option<&str>) -> Result<()> {
        self.session = None;
        if let Some(pw) = password {
            return self.connect(pw).await;
        }
        let secrets = crate::auth::load_account_secrets(&self.account.auth_vault_key)
            .map_err(|e| StoreError::NotFound(format!("keyring secret: {e}")))?;
        self.connect(&secrets.imap_password).await
    }

    /// Liveness probe for pooled sessions: one NOOP round-trip. `false` =
    /// dead, half-closed, or never connected — the caller should drop this
    /// session and connect fresh rather than send real work into it.
    pub async fn is_healthy(&mut self) -> bool {
        match self.session.as_mut() {
            Some(s) => s.noop().await.is_ok(),
            None => false,
        }
    }

    /// Cheap synchronous presence check for `Drop`/`checkin` paths that
    /// cannot await a NOOP round-trip. Staleness is detected at checkout
    /// via [`Self::is_healthy`].
    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    pub(crate) fn session(&mut self) -> Result<&mut ImapSession> {
        self.session
            .as_mut()
            .ok_or_else(|| StoreError::Network("session disconnected".to_string()))
    }

    pub async fn capabilities_list(&mut self) -> Result<Vec<String>> {
        let session = self.session()?;
        session.capability().await
    }

    pub async fn search_server_into_cache(
        &mut self,
        db: &Db,
        account_id: i64,
        tokens: &[String],
        folder_scope: Option<&str>,
    ) -> Result<ServerSearchReport> {
        const PER_FOLDER_CAP: usize = 50;
        const TOTAL_CAP: u64 = 100;
        let mut report = ServerSearchReport::default();
        let ascii: Vec<&str> = tokens
            .iter()
            .map(String::as_str)
            .filter(|t| t.is_ascii())
            .collect();
        if ascii.is_empty() {
            return Ok(report);
        }
        let account = accounts::get(db, account_id)?;
        let targets: Vec<_> = folders::list_by_account(db, account_id)?
            .into_iter()
            .filter(|f| folder_scope.map(|s| f.path == s).unwrap_or(true))
            .collect();
        for folder in targets {
            if report.fetched >= TOTAL_CAP {
                break;
            }
            let session = self.session()?;
            if session.select(&folder.path, None).await.is_err() {
                log::debug!("search: cannot select {}", folder.path);
                continue;
            }
            report.folders_searched += 1;
            let mut hits: Option<HashSet<u32>> = None;
            let mut failed = false;
            for tok in &ascii {
                let astring = match AString::try_from(tok.to_string()) {
                    Ok(a) => a,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                };
                match session.uid_search(vec1![SearchKey::Text(astring)]).await {
                    Ok(uids) => {
                        let set: HashSet<u32> = uids.into_iter().collect();
                        hits = Some(match hits {
                            Some(h) => h.intersection(&set).copied().collect(),
                            None => set,
                        });
                    }
                    Err(e) => {
                        log::debug!("search: {} TEXT query failed: {e}", folder.path);
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                continue;
            }
            let local: HashSet<u32> = messages::list_uids(db, folder.id)?.into_iter().collect();
            let mut missing: Vec<u32> = hits
                .unwrap_or_default()
                .difference(&local)
                .copied()
                .collect();
            missing.sort_unstable_by(|a, b| b.cmp(a));
            missing.truncate(PER_FOLDER_CAP);
            for chunk in missing.chunks(FETCH_CHUNK) {
                if report.fetched >= TOTAL_CAP {
                    break;
                }
                let fetched = session.uid_fetch_messages(chunk).await?;
                for (uid, flags, raw) in fetched {
                    let (parsed, files) =
                        parse_to_new(account.id, folder.id, uid, &flags, &raw, false)?;
                    let id = messages::upsert(db, &parsed)?;
                    collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
                    store_attachment_meta(db, id, files);
                    report.fetched += 1;
                }
            }
        }
        Ok(report)
    }

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
        session.expunge().await?;
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
        session.expunge().await?;
        let count = messages::delete_many_by_uids(db, folder_id, uids)?;
        Ok(count)
    }

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
        if let Some(validity) = mb.uid_validity {
            if folder.uid_validity.is_some_and(|v| v != validity) {
                log::warn!("imap: UIDVALIDITY changed for {} — resyncing", folder.path);
                messages::delete_by_folder(db, folder_id)?;
            }
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
        let new_modseq = mb.highest_modseq.unwrap_or(folder.highest_modseq);
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
    pub async fn push_dirty_flags(&mut self, db: &Db, account_id: i64) -> u64 {
        let mut pushed = 0u64;
        for m in messages::list_flags_dirty(db, account_id).unwrap_or_default() {
            if self.push_flags(db, &m).await.is_ok() {
                let _ = messages::clear_flags_dirty(db, m.id);
                pushed += 1;
            }
        }
        pushed
    }
}

impl SyncProvider for ImapSync {
    fn name(&self) -> &'static str {
        "imap"
    }

    async fn sync_folders(&mut self, db: &Db, account_id: i64) -> Result<Vec<Folder>> {
        discover_folders(self.session()?, db, account_id).await
    }

    async fn sync_folder(&mut self, db: &Db, folder_id: i64) -> Result<SyncReport> {
        self.sync_folder_window(db, folder_id, Some(FULL_SYNC_WINDOW))
            .await
    }

    async fn push_flags(&mut self, db: &Db, message: &Message) -> Result<()> {
        let folder = folders::get(db, message.folder_id)?;
        let session = self.session()?;
        session.select(&folder.path, None).await?;

        let seen_op = if message.is_read {
            StoreType::Add
        } else {
            StoreType::Remove
        };
        session
            .uid_store_flags(&[message.uid], seen_op, vec![Flag::Seen])
            .await?;

        let star_op = if message.is_starred {
            StoreType::Add
        } else {
            StoreType::Remove
        };
        session
            .uid_store_flags(&[message.uid], star_op, vec![Flag::Flagged])
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_session_is_not_healthy() {
        let a = crate::models::Account {
            id: 1,
            name: "n".to_string(),
            email_address: "e".to_string(),
            from_name: String::new(),
            imap_host: "h".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "u".to_string(),
            smtp_host: "s".to_string(),
            smtp_port: 465,
            smtp_security: "tls".to_string(),
            smtp_username: "u".to_string(),
            auth_vault_key: "k".to_string(),
            check_interval_secs: 300,
            created_at: "t".to_string(),
            updated_at: "t".to_string(),
        };
        let s = ImapSync::new(&a);
        assert!(!s.is_connected());
    }

    #[tokio::test]
    async fn fresh_session_is_not_healthy_async() {
        let a = crate::models::Account {
            id: 1,
            name: "n".to_string(),
            email_address: "e".to_string(),
            from_name: String::new(),
            imap_host: "h".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "u".to_string(),
            smtp_host: "s".to_string(),
            smtp_port: 465,
            smtp_security: "tls".to_string(),
            smtp_username: "u".to_string(),
            auth_vault_key: "k".to_string(),
            check_interval_secs: 300,
            created_at: "t".to_string(),
            updated_at: "t".to_string(),
        };
        let mut s = ImapSync::new(&a);
        assert!(!s.is_healthy().await);
    }
}
