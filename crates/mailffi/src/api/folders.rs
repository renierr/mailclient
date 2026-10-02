//! Folder tree reads and the display-only subscription toggle.
//!
//! Everything that talks to the server (LIST refresh, folder creation) lives
//! in [`crate::api::sync`] and [`crate::api::mutate`] instead, because it has
//! to be queued rather than answered inline.

use mailcore::feed;
use mailcore::store::folders;

use crate::db::shared_db;

/// The account's folder tree as JSON: `[{id, name, role, unread, count,
/// subscribed, delimiter}]`. `name` is the full IMAP path; the sidebar
/// derives depth from it by splitting on `delimiter`.
pub fn folders_json(account_id: i64) -> anyhow::Result<String> {
    Ok(mailcore::feed::folders_json(shared_db()?, account_id)?)
}

/// Resolve a folder path to its local id, for a UI that navigated by path.
pub fn folder_id_for_path(account_id: i64, path: String) -> anyhow::Result<i64> {
    Ok(folders::get_by_path(shared_db()?, account_id, &path)?.id)
}

/// The IMAP path of a folder, for the calls that address folders by path.
pub fn folder_path(folder_id: i64) -> anyhow::Result<String> {
    Ok(folders::get(shared_db()?, folder_id)?.path)
}

/// Show or hide a folder in the sidebar.
///
/// Display-only: a hidden folder keeps its cache, still quick-syncs so its
/// unread pill stays honest, and an explicit open still syncs it.
pub fn set_folder_subscribed(folder_id: i64, subscribed: bool) -> anyhow::Result<()> {
    Ok(folders::set_subscribed(
        shared_db()?,
        folder_id,
        subscribed,
    )?)
}

/// How many messages this folder holds locally, how many the server last
/// reported (`-1`: never selected), and what its "Show older" row says
/// (`mailcore::feed::older_state`).
pub fn folder_counts(folder_id: i64) -> anyhow::Result<FolderCounts> {
    let db = shared_db()?;
    let cached = mailcore::store::messages::count_by_folder(db, folder_id)?;
    let server = folders::get(db, folder_id)
        .ok()
        .and_then(|f| f.server_total);
    let older = feed::older_state(cached, server);
    Ok(FolderCounts {
        cached: cached as i64,
        server: server.map_or(-1, |s| s as i64),
        older: match older {
            feed::OlderState::Unchecked => OlderState::Unchecked,
            feed::OlderState::Partial => OlderState::Partial,
            feed::OlderState::Empty => OlderState::Empty,
            feed::OlderState::Complete => OlderState::Complete,
        },
        can_load_older: older.can_load(),
    })
}

#[flutter_rust_bridge::frb]
#[derive(Clone, Copy, Debug)]
pub struct FolderCounts {
    pub cached: i64,
    /// `-1` when the server has not reported a count yet.
    pub server: i64,
    pub older: OlderState,
    /// Asking the server could bring more mail.
    pub can_load_older: bool,
}

/// What a folder's "Show older" row says.
#[derive(Clone, Copy, Debug)]
pub enum OlderState {
    /// The server never reported a count: offer to ask it.
    Unchecked,
    /// The server holds more than the cache.
    Partial,
    /// Nothing here, on either side.
    Empty,
    /// Everything the server has is cached.
    Complete,
}
