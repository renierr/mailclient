//! Multi-pass folder discovery: a single `LIST "" "*"` misses folders on
//! servers with restricted LIST output or namespace gaps.
//!
//! Pass 1 = full recursive LIST, pass 2 = LSUB merge (subscribed folders some
//! servers only report there), pass 3 = per-root subtree LIST for namespace
//! roots the bare `"*"` didn't expand (both the reported delimiter and `"."`
//! — Tobit David uses dotted prefixes), pass 4 = LIST inside every NAMESPACE
//! prefix (personal/other/shared). First pass wins role mapping (it carries
//! SPECIAL-USE); later passes only add unseen names. Auxiliary passes never
//! fail sync.

use std::collections::HashSet;

use crate::db::Db;
use crate::error::Result;
use crate::models::{Folder, FolderRole};
use crate::store::folders;

use super::{
    roles::{attr_text, is_selectable, map_folder_role, role_from_name},
    session::ImapSession,
};

/// Full folder discovery at most this often; in between, a single LIST
/// pass suffices when it matches the cache (folder trees barely change,
/// but every auto-sync paid all four discovery passes before the first
/// folder — the slowest part of a Gmail sync start).
pub(crate) const FULL_DISCOVERY_INTERVAL_SECS: i64 = 2 * 3600;

/// Pure decision: full discovery when the quick LIST disagrees with the
/// cache (new/renamed server folders show up immediately) or the last
/// full run is older than the interval (backstop for folders a bare LIST
/// never reports, Tobit-style). `None` last-full means "never": go full.
#[must_use]
pub(crate) fn full_discovery_due(
    cached_paths: &std::collections::HashSet<String>,
    quick_paths: &std::collections::HashSet<String>,
    last_full_unix: Option<i64>,
    now_unix: i64,
) -> bool {
    if cached_paths != quick_paths {
        return true;
    }
    last_full_unix.is_none_or(|t| now_unix.saturating_sub(t) >= FULL_DISCOVERY_INTERVAL_SECS)
}

/// Run all discovery passes and upsert the merged folder list.
pub(crate) async fn discover_folders(
    session: &mut ImapSession,
    db: &Db,
    account_id: i64,
) -> Result<Vec<Folder>> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut discovered: Vec<(String, String, FolderRole)> = Vec::new();
    let mut consider = |name: &str, delimiter: &str, role: FolderRole, attrs: &str, pass: &str| {
        if seen.insert(name.to_string()) {
            log::info!(
                "imap: [{pass}] [{attrs}] delim={delimiter:?} {name} -> {}",
                role.as_str()
            );
            discovered.push((name.to_string(), delimiter.to_string(), role));
        }
    };

    let mut list_count = 0usize;
    let mut lsub_count = 0usize;
    let mut subtree_count = 0usize;

    // Pass 1: LIST "" "*"
    let names = session.list("", "*").await?;
    for n in &names {
        if !is_selectable(&n.attributes) {
            log::debug!("imap: skipping non-selectable {}", n.name);
            continue;
        }
        consider(
            &n.name,
            &n.delimiter,
            map_folder_role(&n.attributes, &n.name),
            &attr_text(&n.attributes),
            "LIST",
        );
        list_count += 1;
    }

    // Pass 2: LSUB "" "*" (best effort).
    match session.lsub("", "*").await {
        Ok(subs) => {
            for n in &subs {
                if !is_selectable(&n.attributes) {
                    continue;
                }
                consider(
                    &n.name,
                    &n.delimiter,
                    role_from_name(&n.name),
                    &attr_text(&n.attributes),
                    "LSUB",
                );
                lsub_count += 1;
            }
        }
        Err(e) => log::warn!("imap: LSUB failed, continuing with LIST results: {e}"),
    }

    // Pass 3: subtree LIST per top-level root (best effort, capped).
    // Both the reported delimiter and "." are tried: Tobit David serves
    // dotted hierarchies (INBOX.Archive) that a "/"-joined pattern misses.
    match session.list("", "%").await {
        Ok(roots) => {
            for root in roots.iter().take(64) {
                let delim = root.delimiter.as_str();
                let base = root.name.as_str();
                if base.is_empty() {
                    continue;
                }
                let join = |d: &str| {
                    if base.ends_with(d) {
                        format!("{base}*")
                    } else {
                        format!("{base}{d}*")
                    }
                };
                let mut patterns = vec![join(delim)];
                if delim != "." {
                    patterns.push(join("."));
                }
                for pattern in patterns {
                    match session.list("", &pattern).await {
                        Ok(children) => {
                            for n in children.iter() {
                                if !is_selectable(&n.attributes) {
                                    continue;
                                }
                                consider(
                                    &n.name,
                                    &n.delimiter,
                                    map_folder_role(&n.attributes, &n.name),
                                    &attr_text(&n.attributes),
                                    "SUBTREE",
                                );
                                subtree_count += 1;
                            }
                        }
                        Err(e) => log::debug!("imap: subtree LIST {pattern} failed: {e}"),
                    }
                }
            }
        }
        Err(e) => log::debug!("imap: root LIST failed, skipping subtree pass: {e}"),
    }

    // Pass 4: LIST inside every NAMESPACE prefix (best effort, capped).
    // Shared / other-users' branches live outside "" and never appear in
    // passes 1–3; the server tells us where via RFC 2342 (when it bothers).
    // The empty personal prefix is pass 1 again, so it is skipped.
    let mut ns_count = 0usize;
    match session.namespace().await {
        Ok((personal, other, shared)) => {
            for prefix in personal
                .iter()
                .chain(other.iter())
                .chain(shared.iter())
                .filter(|p| !p.is_empty())
                .take(12)
            {
                match session.list(prefix, "*").await {
                    Ok(extra) => {
                        for n in extra.iter() {
                            if !is_selectable(&n.attributes) {
                                continue;
                            }
                            consider(
                                &n.name,
                                &n.delimiter,
                                map_folder_role(&n.attributes, &n.name),
                                &attr_text(&n.attributes),
                                "NAMESPACE",
                            );
                            ns_count += 1;
                        }
                    }
                    Err(e) => log::debug!("imap: namespace LIST {prefix:?} failed: {e}"),
                }
            }
        }
        Err(e) => log::debug!("imap: NAMESPACE query failed, skipping pass 4: {e}"),
    }

    log::info!(
            "imap: discovery LIST*={list_count} LSUB={lsub_count} subtrees={subtree_count} namespaces={ns_count} merged={}",
            discovered.len()
        );
    let out = upsert_discovered(db, account_id, &discovered)?;
    // Stamp the full run so throttled auto-syncs can skip passes 2-4 until
    // the interval lapses (manual refreshes stamp here too — they call this).
    if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        let _ =
            crate::store::settings::set_last_full_discovery(db, account_id, now.as_secs() as i64);
    }
    log::info!("imap: {} folders", out.len());
    Ok(out)
}

/// Quick refresh: pass 1 (`LIST "" "*"`) only, upserted the same way.
/// The auto-sync path runs this every time and escalates to the full
/// discovery only when it disagrees with the cache or the interval lapsed
/// (see [`full_discovery_due`]) — one round-trip instead of ~30 in the
/// common case. Never stamps: only full runs move the timestamp.
pub(crate) async fn discover_folders_quick(
    session: &mut ImapSession,
    db: &Db,
    account_id: i64,
) -> Result<Vec<Folder>> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut discovered: Vec<(String, String, FolderRole)> = Vec::new();
    for n in session.list("", "*").await?.iter() {
        if !is_selectable(&n.attributes) {
            continue;
        }
        if seen.insert(n.name.clone()) {
            log::info!(
                "imap: [LIST] [{}] delim={:?} {} -> {}",
                attr_text(&n.attributes),
                n.delimiter,
                n.name,
                map_folder_role(&n.attributes, &n.name).as_str()
            );
            discovered.push((
                n.name.clone(),
                n.delimiter.clone(),
                map_folder_role(&n.attributes, &n.name),
            ));
        }
    }
    upsert_discovered(db, account_id, &discovered)
}

/// Insert or refresh the merged folder list. Shared by full and quick
/// discovery; never touches visibility (`subscribed`) or sync state.
fn upsert_discovered(
    db: &Db,
    account_id: i64,
    discovered: &[(String, String, FolderRole)],
) -> Result<Vec<Folder>> {
    let mut out = Vec::new();
    for (path, delimiter, role) in discovered {
        let id = folders::upsert(db, account_id, path, delimiter, *role)?;
        out.push(folders::get(db, id)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> std::collections::HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unchanged_tree_with_fresh_stamp_skips_full_discovery() {
        let cached = paths(&["INBOX", "Sent"]);
        assert!(!full_discovery_due(&cached, &cached, Some(1000), 1000 + 60));
    }

    #[test]
    fn changed_tree_triggers_full_discovery_at_once() {
        let cached = paths(&["INBOX"]);
        let quick = paths(&["INBOX", "Sent"]);
        assert!(full_discovery_due(&cached, &quick, Some(1000), 1000 + 60));
    }

    #[test]
    fn stale_stamp_triggers_full_discovery_despite_match() {
        let cached = paths(&["INBOX"]);
        assert!(full_discovery_due(
            &cached,
            &cached,
            Some(1000),
            1000 + FULL_DISCOVERY_INTERVAL_SECS + 1
        ));
        // Never ran: always full.
        assert!(full_discovery_due(&cached, &cached, None, 2000));
    }
}
