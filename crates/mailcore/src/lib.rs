//! mailcore — pure-Rust backend of mailclient.
//!
//! No Qt dependency here (see AGENT.md). The Qt/QML bridge lives in `mailapp`
//! and talks to this crate.

pub mod db;
pub mod error;
pub mod models;
pub mod search;
pub mod store;
pub mod sync;

pub use db::{default_db_path, Db};
pub use error::{Result, StoreError};
