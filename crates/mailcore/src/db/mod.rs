//! SQLite database handle.

pub mod migrations;
pub mod schema {
    //! Re-export of the raw DDL for tooling/tests.
    pub const SCHEMA_SQL: &str = include_str!("schema.sql");
}

use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;

/// Default on-disk location: `~/.local/share/mailclient/mailclient.sqlite`
/// (override with `MAILCLIENT_DB`).
#[must_use]
pub fn default_db_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("MAILCLIENT_DB") {
        return std::path::PathBuf::from(p);
    }
    directories::ProjectDirs::from("org", "omarchy", "mailclient")
        .map(|d| d.data_dir().join("mailclient.sqlite"))
        .unwrap_or_else(|| std::path::PathBuf::from("mailclient.sqlite"))
}

/// Thin wrapper around a rusqlite connection with our schema applied.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (creating parent dirs) and migrate to the current schema.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    crate::error::StoreError::InvalidInput(format!(
                        "cannot create db dir {}: {e}",
                        parent.display()
                    ))
                })?;
            }
        }
        let conn = Connection::open(path)?;
        Self::configure(&conn)?;
        migrations::ensure_schema(&conn)?;
        Ok(Self { conn })
    }

    /// In-memory DB, used by unit tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        migrations::ensure_schema(&conn)?;
        Ok(Self { conn })
    }

    fn configure(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "pragma journal_mode = WAL;
             pragma synchronous = NORMAL;
             pragma foreign_keys = ON;",
        )?;
        Ok(())
    }

    /// Direct access to the underlying connection for stores.
    pub const fn conn(&self) -> &Connection {
        &self.conn
    }
}
