//! Server-side SEARCH, used to top up thin local FTS results with mail
//! that was never cached.

use super::*;

use crate::search::{parse_query_full, ParsedQuery, SearchField, SearchTerm};
use imap_types::core::Vec1;

impl ImapSync {
    /// `query` is read by [`parse_query_full`] like the local search, and sent as
    /// one SEARCH per folder (see [`imap_criteria`]).
    pub async fn search_server_into_cache(
        &mut self,
        db: &Db,
        account_id: i64,
        query: &str,
        folder_scope: Option<&str>,
    ) -> Result<ServerSearchReport> {
        const PER_FOLDER_CAP: usize = 50;
        const TOTAL_CAP: u64 = 100;
        let mut report = ServerSearchReport::default();
        let Some(criteria) = imap_criteria(&parse_query_full(query)) else {
            return Ok(report);
        };
        let account = accounts::get(db, account_id)?;
        let targets: Vec<_> = folders::list_by_account(db, account_id)?
            .into_iter()
            .filter(|f| folder_scope.map(|s| f.path == s).unwrap_or(true))
            .collect();
        for folder in targets {
            if report.fetched >= TOTAL_CAP {
                break;
            }
            let session = self.session()?;
            if session.select(&folder.path, None).await.is_err() {
                log::debug!("search: cannot select {}", folder.path);
                continue;
            }
            report.folders_searched += 1;
            let hits: HashSet<u32> = match session.uid_search(criteria.clone()).await {
                Ok(uids) => uids.into_iter().collect(),
                Err(e) => {
                    log::debug!("search: {} SEARCH failed: {e}", folder.path);
                    continue;
                }
            };
            let local: HashSet<u32> = messages::list_uids(db, folder.id)?.into_iter().collect();
            let mut missing: Vec<u32> = hits.difference(&local).copied().collect();
            missing.sort_unstable_by(|a, b| b.cmp(a));
            missing.truncate(PER_FOLDER_CAP);
            for chunk in missing.chunks(FETCH_CHUNK) {
                if report.fetched >= TOTAL_CAP {
                    break;
                }
                let fetched = session.uid_fetch_messages(chunk).await?;
                for (uid, flags, raw) in fetched {
                    let (parsed, files) =
                        parse_to_new(account.id, folder.id, uid, &flags, &raw, false)?;
                    let id = messages::upsert(db, &parsed)?;
                    collect_contacts_from_headers(
                        db,
                        parsed.account_id,
                        parsed.raw_headers.as_deref(),
                    );
                    store_attachment_meta(db, id, files);
                    report.fetched += 1;
                }
            }
        }
        Ok(report)
    }
}

/// The SEARCH keys for `terms` and filter tokens (the server ANDs them): words and phrases
/// are `TEXT`, `from:` is `FROM`, `to:` is any of `TO`/`CC`/`BCC`,
/// `subject:` is `SUBJECT`, exclusions wrap their key in `NOT`,
/// `is:unread` maps to `UNSEEN`, `is:starred` maps to `FLAGGED`, etc.
/// Non-ASCII terms stay local-only (servers disagree on SEARCH charsets).
/// `None` when no positive term is left to send.
fn imap_criteria(parsed: &ParsedQuery) -> Option<Vec1<SearchKey<'static>>> {
    let mut keys: Vec<(bool, SearchKey<'static>)> = parsed
        .terms
        .iter()
        .filter_map(|t| Some((t.negated, imap_key(t)?)))
        .collect();

    if let Some(unread) = parsed.filters.unread {
        let key = if unread {
            SearchKey::Unseen
        } else {
            SearchKey::Seen
        };
        keys.push((false, key));
    }

    if let Some(starred) = parsed.filters.starred {
        let key = if starred {
            SearchKey::Flagged
        } else {
            SearchKey::Unflagged
        };
        keys.push((false, key));
    }

    if let Some(ref after) = parsed.filters.after {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(after, "%Y-%m-%d") {
            if let Ok(imap_d) = imap_types::datetime::NaiveDate::try_from(d) {
                keys.push((false, SearchKey::Since(imap_d)));
            }
        }
    }

    if let Some(ref before) = parsed.filters.before {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(before, "%Y-%m-%d") {
            if let Ok(imap_d) = imap_types::datetime::NaiveDate::try_from(d) {
                keys.push((false, SearchKey::Before(imap_d)));
            }
        }
    }

    if keys.is_empty() || keys.iter().all(|(negated, _)| *negated) {
        return None;
    }
    Vec1::try_from(keys.into_iter().map(|(_, k)| k).collect::<Vec<_>>()).ok()
}

fn imap_key(t: &SearchTerm) -> Option<SearchKey<'static>> {
    if !t.text.is_ascii() {
        return None;
    }
    let text = AString::try_from(t.text.clone()).ok()?;
    let key = match t.field {
        SearchField::Any => SearchKey::Text(text),
        SearchField::From => SearchKey::From(text),
        SearchField::Subject => SearchKey::Subject(text),
        SearchField::To => SearchKey::Or(
            Box::new(SearchKey::To(text.clone())),
            Box::new(SearchKey::Or(
                Box::new(SearchKey::Cc(text.clone())),
                Box::new(SearchKey::Bcc(text)),
            )),
        ),
    };
    Some(if t.negated {
        SearchKey::Not(Box::new(key))
    } else {
        key
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn criteria(raw: &str) -> Option<Vec<SearchKey<'static>>> {
        imap_criteria(&parse_query_full(raw)).map(Vec1::into_inner)
    }

    fn astr(s: &str) -> AString<'static> {
        AString::try_from(s.to_string()).unwrap()
    }

    #[test]
    fn terms_map_to_search_keys() {
        assert_eq!(
            criteria(r#"invoice "project plan" from:anna subject:q3 -reminder"#).unwrap(),
            vec![
                SearchKey::Text(astr("invoice")),
                SearchKey::Text(astr("project plan")),
                SearchKey::From(astr("anna")),
                SearchKey::Subject(astr("q3")),
                SearchKey::Not(Box::new(SearchKey::Text(astr("reminder")))),
            ]
        );
        assert_eq!(
            criteria("to:bob").unwrap(),
            vec![SearchKey::Or(
                Box::new(SearchKey::To(astr("bob"))),
                Box::new(SearchKey::Or(
                    Box::new(SearchKey::Cc(astr("bob"))),
                    Box::new(SearchKey::Bcc(astr("bob"))),
                )),
            )]
        );
        assert_eq!(
            criteria("is:unread is:starred after:2026-06-01").unwrap(),
            vec![
                SearchKey::Unseen,
                SearchKey::Flagged,
                SearchKey::Since(
                    imap_types::datetime::NaiveDate::try_from(
                        chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
                    )
                    .unwrap(),
                ),
            ]
        );
    }

    #[test]
    fn nothing_positive_to_send_is_none() {
        assert_eq!(criteria(""), None);
        assert_eq!(criteria("-reminder"), None);
        // Non-ASCII stays local; with nothing else the server is not asked.
        assert_eq!(criteria("müller"), None);
        assert_eq!(
            criteria("müller invoice").unwrap(),
            vec![SearchKey::Text(astr("invoice"))]
        );
    }
}
