//! Error type for mailcore.

use thiserror::Error;

/// All errors produced by mailcore.
#[derive(Debug, Error)]
pub enum StoreError {
    /// SQLite failure.
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    /// JSON (de)serialization failure for JSON-encoded columns.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// Requested row does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// Invalid input (bad address, bad folder path, ...).
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// Time handling failure.
    #[error("time error: {0}")]
    Time(#[from] chrono::ParseError),
    /// IMAP protocol/transport failure.
    #[error("imap error: {0}")]
    Imap(#[from] imap::Error),
    /// TLS failure.
    #[error("tls error: {0}")]
    Tls(#[from] native_tls::Error),
    /// SMTP failure.
    #[error("smtp error: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
    /// Address parse failure.
    #[error("address error: {0}")]
    Address(#[from] lettre::address::AddressError),
    /// lettre message build failure.
    #[error("message build error: {0}")]
    Lettre(#[from] lettre::error::Error),
    /// I/O failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience alias.
pub type Result<T, E = StoreError> = std::result::Result<T, E>;
