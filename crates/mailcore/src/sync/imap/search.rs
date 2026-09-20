//! UID windowing: cover the newest-N mail without enumerating huge mailboxes.

use std::collections::HashSet;

use imap_types::{search::SearchKey, sequence::SequenceSet};

use crate::error::{Result, StoreError};

use super::{session::ImapSession, vec1};

/// SEARCH newest UIDs within window.
pub(crate) async fn search_recent_uids(
    session: &mut ImapSession,
    window: Option<usize>,
    uid_next: Option<u32>,
) -> Result<(HashSet<u32>, u32)> {
    let Some(n) = window else {
        let uids = session.uid_search(vec1![SearchKey::All]).await?;
        return Ok((uids.into_iter().collect(), 1));
    };

    let top = uid_next.unwrap_or(0).saturating_sub(1);
    if top == 0 {
        let uids = session.uid_search(vec1![SearchKey::All]).await?;
        return Ok((uids.into_iter().collect(), 1));
    }

    let span = (n as u32).saturating_mul(8).max(n as u32);
    search_paged(session, top, n, span).await
}

const SEARCH_PAGES: u32 = 8;

/// SEARCH UID space backwards from `top`, paging down until `want` UIDs are
/// known or UID 1 is reached.
async fn search_paged(
    session: &mut ImapSession,
    top: u32,
    want: usize,
    span: u32,
) -> Result<(HashSet<u32>, u32)> {
    let span = span.max(1);
    let mut found = HashSet::new();
    let mut hi = top;
    let mut lo = hi.saturating_sub(span.saturating_sub(1)).max(1);
    for _ in 0..SEARCH_PAGES {
        let seq_str = format!("{lo}:{hi}");
        let seq = SequenceSet::try_from(seq_str.as_str()).map_err(|e| {
            StoreError::InvalidInput(format!("invalid sequence set {seq_str}: {e}"))
        })?;
        let uids = session.uid_search(vec1![SearchKey::Uid(seq)]).await?;
        found.extend(uids);
        if found.len() >= want || lo <= 1 {
            break;
        }
        hi = lo.saturating_sub(1);
        if hi == 0 {
            break;
        }
        lo = hi.saturating_sub(span.saturating_sub(1)).max(1);
    }
    Ok((found, lo))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    #[test]
    fn test_search_window_in_window_logic() {
        let server_uids: HashSet<u32> = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10].into_iter().collect();
        let window = Some(5);
        let relevant: Option<HashSet<u32>> = window.map(|n| {
            let mut sorted: Vec<u32> = server_uids.iter().copied().collect();
            sorted.sort_unstable();
            let skip = sorted.len().saturating_sub(n);
            sorted.into_iter().skip(skip).collect()
        });
        let in_window = |uid: &u32| relevant.as_ref().is_none_or(|r| r.contains(uid));

        for uid in 1..=5 {
            assert!(!in_window(&uid));
        }
        for uid in 6..=10 {
            assert!(in_window(&uid));
        }
    }
}
