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
    let mut out = Vec::new();
    for (path, delimiter, role) in &discovered {
        let id = folders::upsert(db, account_id, path, delimiter, *role)?;
        out.push(folders::get(db, id)?);
    }
    log::info!("imap: {} folders", out.len());
    Ok(out)
}
