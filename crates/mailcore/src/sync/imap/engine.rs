//! [`ImapSync`]: high-level sync orchestration over [`super::session`].
//!
//! Orchestration, grouped by what it is for: [`connect`] owns the connection
//! lifecycle, [`mutate`] turns user actions into server state, [`sync`] keeps
//! the local cache in line with the server, and [`search`] backfills what FTS
//! never cached. Folder discovery lives in [`super::folders`].

mod connect;
mod mutate;
mod search;
mod sync;

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

    pub(crate) fn session(&mut self) -> Result<&mut ImapSession> {
        self.session
            .as_mut()
            .ok_or_else(|| StoreError::Network("session disconnected".to_string()))
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
