//! Search: the local FTS index first, the server only when that is thin.

use crate::db::shared_db;
use crate::net::{spawn, JobRefresh};
use crate::session::{checkout_session, resolve_account};

/// FTS5 search over subject / sender / body for one account.
///
/// Pure SQLite, no network, safe to call on every keystroke. `folder` scopes
/// to one IMAP path; empty searches the whole account. A blank or
/// operator-only query yields `[]` rather than an error.
pub fn search_json(
    account_id: i64,
    query: String,
    folder: String,
    limit: i64,
) -> anyhow::Result<String> {
    Ok(mailcore::feed::search_json(
        shared_db()?,
        account_id,
        &query,
        limit.max(0) as u64,
        &folder,
    )?)
}

/// Backfill thin local results from the server.
///
/// Runs IMAP `TEXT` search per token across the account's folders — or one
/// folder when `folder` is set — and pulls missing hits into the cache
/// (bounded, metadata only). Queued; when the `"Search"` job finishes, re-run
/// [`search_json`] and the new hits are there.
pub fn search_server(account_id: i64, query: String, folder: String) -> anyhow::Result<()> {
    spawn(
        "Search",
        format!("search:{account_id}"),
        move |db, _progress| async move {
            let account = resolve_account(db, account_id)?;
            // `mailcore` searches per token, so the query is split here rather
            // than handed over as one phrase.
            let tokens: Vec<String> = query.split_whitespace().map(str::to_string).collect();
            let scope = (!folder.is_empty()).then_some(folder.as_str());
            let mut imap = checkout_session(&account).await?;
            let report = imap
                .search_server_into_cache(db, account.id, &tokens, scope)
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
