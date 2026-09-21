//! Server-side SEARCH, used to top up thin local FTS results with mail
//! that was never cached.

use super::*;

impl ImapSync {
    pub async fn search_server_into_cache(
        &mut self,
        db: &Db,
        account_id: i64,
        tokens: &[String],
        folder_scope: Option<&str>,
    ) -> Result<ServerSearchReport> {
        const PER_FOLDER_CAP: usize = 50;
        const TOTAL_CAP: u64 = 100;
        let mut report = ServerSearchReport::default();
        let ascii: Vec<&str> = tokens
            .iter()
            .map(String::as_str)
            .filter(|t| t.is_ascii())
            .collect();
        if ascii.is_empty() {
            return Ok(report);
        }
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
            let mut hits: Option<HashSet<u32>> = None;
            let mut failed = false;
            for tok in &ascii {
                let astring = match AString::try_from(tok.to_string()) {
                    Ok(a) => a,
                    Err(_) => {
                        failed = true;
                        break;
                    }
                };
                match session.uid_search(vec1![SearchKey::Text(astring)]).await {
                    Ok(uids) => {
                        let set: HashSet<u32> = uids.into_iter().collect();
                        hits = Some(match hits {
                            Some(h) => h.intersection(&set).copied().collect(),
                            None => set,
                        });
                    }
                    Err(e) => {
                        log::debug!("search: {} TEXT query failed: {e}", folder.path);
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                continue;
            }
            let local: HashSet<u32> = messages::list_uids(db, folder.id)?.into_iter().collect();
            let mut missing: Vec<u32> = hits
                .unwrap_or_default()
                .difference(&local)
                .copied()
                .collect();
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
                    collect_contacts_from_headers(db, parsed.raw_headers.as_deref());
                    store_attachment_meta(db, id, files);
                    report.fetched += 1;
                }
            }
        }
        Ok(report)
    }
}
