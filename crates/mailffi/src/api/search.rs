//! Search: the local FTS index first, the server only when that is thin.

use crate::db::shared_db;
use crate::net::{spawn, JobRefresh};
use mailcore::search;
use mailcore::sync::pool::{checkout_session, resolve_account};

/// FTS5 search over subject / sender / recipients / body for one account.
///
/// Pure SQLite, no network, safe to call on every keystroke. `folder` scopes
/// to one IMAP path; empty searches the whole account. A blank or
/// operator-only query yields `[]` rather than an error. At most
/// [`SearchPlan::hit_limit`] hits.
pub fn search_json(account_id: i64, query: String, folder: String) -> anyhow::Result<String> {
    Ok(mailcore::feed::search_json(
        shared_db()?,
        account_id,
        &query,
        search::HIT_LIMIT,
        &folder,
    )?)
}

/// Query similar messages across the account as JSON (same shape as
/// `search_json`).
pub fn similar_json(account_id: i64, folder_id: i64, uid: i64) -> anyhow::Result<String> {
    Ok(mailcore::similar::similar_json(
        shared_db()?,
        account_id,
        folder_id,
        uid,
        search::HIT_LIMIT,
    )?)
}

/// Target message's subject for the "Similar to: ..." chip.
pub fn similar_subject(account_id: i64, folder_id: i64, uid: i64) -> anyhow::Result<String> {
    Ok(mailcore::similar::target_subject(
        shared_db()?,
        account_id,
        folder_id,
        uid,
    )?)
}

/// How the search field runs `query` (`mailcore::search::plan`).
#[flutter_rust_bridge::frb(sync)]
pub fn search_plan(query: String) -> SearchPlan {
    let p = search::plan(&query);
    SearchPlan {
        mode: match p.mode {
            search::SearchMode::Off => SearchMode::Off,
            search::SearchMode::Filter => SearchMode::Filter,
            search::SearchMode::Index => SearchMode::Indexed,
        },
        query: p.query,
        hit_limit: p.hit_limit as u32,
        debounce_ms: p.debounce_ms as u32,
    }
}

/// The short-input filter over one list row
/// (`mailcore::search::filter_matches`).
#[flutter_rust_bridge::frb(sync)]
pub fn search_filter_matches(
    query: String,
    subject: String,
    from: String,
    from_name: String,
    snippet: String,
) -> bool {
    search::filter_matches(&query, &subject, &from, &from_name, &snippet)
}

/// [`mailcore::search::SearchPlan`] as a generated struct.
pub struct SearchPlan {
    pub mode: SearchMode,
    /// The query as searched: trimmed.
    pub query: String,
    /// Most hits per query; fewer local hits ask the server too.
    pub hit_limit: u32,
    /// Typing pause before a server search.
    pub debounce_ms: u32,
}

/// What the search field does with its text.
pub enum SearchMode {
    /// Nothing typed.
    Off,
    /// One or two letters: filter the shown folder.
    Filter,
    /// Three and more: the index, topped up from the server. (`Index`
    /// in the core; Dart enums cannot name a value `index`.)
    Indexed,
}

/// Backfill thin local results from the server.
///
/// Runs one IMAP SEARCH built from the query (same language as
/// [`search_json`]) across the account's folders — or one folder when
/// `folder` is set — and pulls missing hits into the cache
/// (bounded, metadata only). Queued; when the `"Search"` job finishes, re-run
/// [`search_json`] and the new hits are there.
pub fn search_server(account_id: i64, query: String, folder: String) -> anyhow::Result<()> {
    spawn(
        "Search",
        format!("search:{account_id}"),
        move |db, _progress| async move {
            let account = resolve_account(db, account_id)?;
            let scope = (!folder.is_empty()).then_some(folder.as_str());
            let mut imap = checkout_session(&account).await?;
            let report = imap
                .search_server_into_cache(db, account.id, &query, scope)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            let status = match report.fetched {
                0 => "No further matches on the server".to_string(),
                n => format!("Fetched {n} more match(es) from the server"),
            };
            Ok((status, Some(JobRefresh::account(account.id))))
        },
    )
}
