//! IMAP sync (Milestone 1).
//!
//! M0 only resolves connection parameters from an [`crate::models::Account`]
//! so the UI/account-setup work can proceed; the network session (LOGIN,
//! LIST/SELECT/FETCH, IDLE) lands in M1 using the `imap` + `native-tls` crates.

use crate::error::Result;
use crate::models::{Account, Folder, Message};
use crate::sync::traits::{SyncProvider, SyncReport};

/// Resolved IMAP endpoint for one account.
#[derive(Debug, Clone)]
pub struct ImapEndpoint {
    /// `host:port`.
    pub addr: String,
    /// `true` for implicit TLS (993), `false` for STARTTLS/plain (143 + opt-in).
    pub implicit_tls: bool,
}

/// Derive the endpoint from account settings.
#[must_use]
pub fn endpoint_for(account: &Account) -> ImapEndpoint {
    ImapEndpoint {
        addr: format!("{}:{}", account.imap_host, account.imap_port),
        implicit_tls: account.imap_port == 993 || account.imap_security.eq_ignore_ascii_case("tls"),
    }
}

/// IMAP sync session. Holds no connection yet in M0.
pub struct ImapSync {
    endpoint: ImapEndpoint,
    username: String,
}

impl ImapSync {
    /// Build from account settings (password comes from the OS keyring at connect).
    #[must_use]
    pub fn new(account: &Account) -> Self {
        Self {
            endpoint: endpoint_for(account),
            username: account.imap_username.clone(),
        }
    }

    /// Dial + LOGIN. M1: `native_tls` → `imap::connect`.
    pub fn connect(&mut self, _password: &str) -> Result<()> {
        log::info!(
            "M1: would connect to {} as {}",
            self.endpoint.addr,
            self.username
        );
        Ok(())
    }
}

impl SyncProvider for ImapSync {
    fn name(&self) -> &'static str {
        "imap"
    }

    fn sync_folders(&mut self, _account_id: i64) -> Result<Vec<Folder>> {
        Ok(Vec::new())
    }

    fn sync_folder(&mut self, _folder_id: i64) -> Result<SyncReport> {
        Ok(SyncReport::default())
    }

    fn push_flags(&mut self, _message: &Message) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_defaults_to_implicit_tls_on_993() {
        let a = Account {
            id: 1,
            name: "n".to_string(),
            email_address: "e".to_string(),
            imap_host: "imap.x".to_string(),
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
        let ep = endpoint_for(&a);
        assert_eq!(ep.addr, "imap.x:993");
        assert!(ep.implicit_tls);
    }
}
