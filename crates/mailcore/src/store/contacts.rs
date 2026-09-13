//! `contacts` for address autocomplete with alias support and fuzzy search.

use rusqlite::params;

use crate::db::Db;
use crate::error::Result;
use crate::models::Contact;
use crate::store::now;

/// Record having seen an address (insert or bump counter).
///
/// Seeds `alias` from `name` (the transferred real name) if `alias` is not
/// already populated.
pub fn seen(db: &Db, address: &str, name: Option<&str>) -> Result<()> {
    let addr_clean = address.trim();
    if addr_clean.is_empty() {
        return Ok(());
    }
    let name_clean = name.map(str::trim).filter(|s| !s.is_empty());
    let ts = now();
    db.conn().execute(
        "insert into contacts (address, name, alias, times_seen, last_seen_at)
         values (?1, ?2, ?2, 1, ?3)
         on conflict (address) do update set
            name = coalesce(excluded.name, contacts.name),
            alias = coalesce(contacts.alias, excluded.name, contacts.name),
            times_seen = contacts.times_seen + 1,
            last_seen_at = excluded.last_seen_at",
        params![addr_clean, name_clean, ts],
    )?;
    Ok(())
}

/// Explicitly update or set a contact's custom alias.
pub fn set_alias(db: &Db, address: &str, alias: Option<&str>) -> Result<()> {
    let addr_clean = address.trim();
    if addr_clean.is_empty() {
        return Ok(());
    }
    let alias_clean = alias.map(str::trim).filter(|s| !s.is_empty());
    let ts = now();
    db.conn().execute(
        "insert into contacts (address, name, alias, times_seen, last_seen_at)
         values (?1, null, ?2, 1, ?3)
         on conflict (address) do update set
            alias = ?2",
        params![addr_clean, alias_clean, ts],
    )?;
    Ok(())
}

/// Seed contacts table from existing messages in the database.
pub fn seed_from_messages(db: &Db) -> Result<usize> {
    seed_contacts_from_connection(db.conn())
}

/// Seed contacts from connection (used also during schema migrations).
pub fn seed_contacts_from_connection(conn: &rusqlite::Connection) -> Result<usize> {
    let mut stmt = conn.prepare(
        "select raw_headers, from_addr, to_addrs, date from messages
         where raw_headers is not null or from_addr is not null or to_addrs != '[]'",
    )?;

    let mut found: Vec<(String, Option<String>, String)> = Vec::new();

    let rows = stmt.query_map([], |row| {
        let raw_headers: Option<String> = row.get(0)?;
        let from_addr: Option<String> = row.get(1)?;
        let to_addrs: Option<String> = row.get(2)?;
        let date: Option<String> = row.get(3)?;
        Ok((raw_headers, from_addr, to_addrs, date))
    })?;

    for row_res in rows {
        let (raw_headers, from_addr, to_addrs_json, date) = row_res?;
        let ts = date.unwrap_or_else(now);

        let mut parsed_any = false;
        if let Some(headers) = raw_headers.as_deref() {
            if !headers.trim().is_empty() {
                if let Some(parsed) =
                    mail_parser::MessageParser::default().parse(headers.as_bytes())
                {
                    parsed_any = true;
                    let mut collect = |addr_list: Option<&mail_parser::Address>| {
                        if let Some(addrs) = addr_list {
                            for a in addrs.iter() {
                                if let Some(email) = a.address.as_deref() {
                                    let email_clean = email.trim();
                                    if !email_clean.is_empty() && email_clean.contains('@') {
                                        let name_clean = a
                                            .name
                                            .as_deref()
                                            .map(str::trim)
                                            .filter(|s| !s.is_empty())
                                            .map(str::to_string);
                                        found.push((
                                            email_clean.to_string(),
                                            name_clean,
                                            ts.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                    };
                    collect(parsed.from());
                    collect(parsed.to());
                    collect(parsed.cc());
                }
            }
        }

        if !parsed_any {
            if let Some(from) = from_addr {
                let clean = from.trim();
                if !clean.is_empty() && clean.contains('@') {
                    found.push((clean.to_string(), None, ts.clone()));
                }
            }
            if let Some(to_json) = to_addrs_json {
                if let Ok(addrs) = serde_json::from_str::<Vec<String>>(&to_json) {
                    for addr in addrs {
                        let clean = addr.trim();
                        if !clean.is_empty() && clean.contains('@') {
                            found.push((clean.to_string(), None, ts.clone()));
                        }
                    }
                }
            }
        }
    }

    let mut seeded = 0;
    let mut upsert_stmt = conn.prepare(
        "insert into contacts (address, name, alias, times_seen, last_seen_at)
         values (?1, ?2, ?2, 1, ?3)
         on conflict (address) do update set
            name = coalesce(excluded.name, contacts.name),
            alias = coalesce(contacts.alias, excluded.name, contacts.name),
            times_seen = contacts.times_seen + 1,
            last_seen_at = max(contacts.last_seen_at, excluded.last_seen_at)",
    )?;

    for (addr, name, ts) in found {
        upsert_stmt.execute(params![addr, name, ts])?;
        seeded += 1;
    }

    Ok(seeded)
}

/// Match subsequence in target, scoring consecutive matches and start positions.
fn fuzzy_subsequence(query: &str, target: &str) -> Option<i64> {
    let mut q_chars = query.chars().peekable();
    let mut score = 100i64;
    let mut prev_matched_idx: Option<usize> = None;
    let mut consecutive = 0;

    for (i, t_ch) in target.char_indices() {
        if let Some(&q_ch) = q_chars.peek() {
            if t_ch == q_ch {
                q_chars.next();
                if let Some(prev) = prev_matched_idx {
                    if prev + 1 == i {
                        consecutive += 1;
                        score += 15 * consecutive;
                    } else {
                        consecutive = 0;
                        score -= 2;
                    }
                }
                prev_matched_idx = Some(i);
            }
        }
    }

    if q_chars.peek().is_none() {
        Some(score.max(10))
    } else {
        None
    }
}

/// Score a single candidate string field against query.
fn score_field(query: &str, field: &str, weight: f64) -> Option<i64> {
    let f = field.trim().to_lowercase();
    let q = query.trim().to_lowercase();
    if f.is_empty() || q.is_empty() {
        return None;
    }

    let base_score = if f == q {
        1000
    } else if f.starts_with(&q) {
        700 + (100 - f.len().min(100)) as i64 / 2
    } else if f
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|w| w.starts_with(&q))
    {
        550
    } else if f.contains(&q) {
        350
    } else {
        fuzzy_subsequence(&q, &f)?
    };

    Some((base_score as f64 * weight) as i64)
}

/// Calculate fuzzy match score for a contact across alias, domain, domain stem,
/// name, address, and local part.
fn match_score(query: &str, contact: &Contact) -> Option<i64> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(contact.times_seen as i64);
    }

    let alias = contact.alias.as_deref().unwrap_or("");
    let name = contact.name.as_deref().unwrap_or("");
    let addr = contact.address.as_str();
    let (local_part, domain) = addr.split_once('@').unwrap_or((addr, ""));
    let domain_stem = domain.split('.').next().unwrap_or(domain);

    let mut best_score: Option<i64> = None;
    let mut update = |opt: Option<i64>| {
        if let Some(s) = opt {
            best_score = Some(best_score.map_or(s, |curr| curr.max(s)));
        }
    };

    // User-assigned alias has highest weight, then domain stem/domain, name, and address.
    update(score_field(&q, alias, 1.3));
    update(score_field(&q, name, 1.1));
    update(score_field(&q, domain_stem, 1.25));
    update(score_field(&q, domain, 1.1));
    update(score_field(&q, addr, 1.0));
    update(score_field(&q, local_part, 1.0));

    best_score.map(|s| s + (contact.times_seen.min(50) as i64) * 2)
}

/// Top matches for `query` (matching alias, domain, name, or address),
/// ranked by fuzzy match quality and frequency.
pub fn suggest(db: &Db, query: &str, limit: u64) -> Result<Vec<Contact>> {
    let q = query.trim();
    if q.is_empty() {
        let mut stmt = db.conn().prepare(
            "select address, name, alias, times_seen, last_seen_at from contacts
             order by times_seen desc, last_seen_at desc limit ?1",
        )?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(Contact {
                    address: row.get(0)?,
                    name: row.get(1)?,
                    alias: row.get(2)?,
                    times_seen: row.get::<_, i64>(3)? as u64,
                    last_seen_at: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        return Ok(rows);
    }

    let mut stmt = db
        .conn()
        .prepare("select address, name, alias, times_seen, last_seen_at from contacts")?;
    let candidates = stmt
        .query_map([], |row| {
            Ok(Contact {
                address: row.get(0)?,
                name: row.get(1)?,
                alias: row.get(2)?,
                times_seen: row.get::<_, i64>(3)? as u64,
                last_seen_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut scored: Vec<(i64, Contact)> = candidates
        .into_iter()
        .filter_map(|c| match_score(q, &c).map(|score| (score, c)))
        .collect();

    scored.sort_by(|(s1, c1), (s2, c2)| {
        s2.cmp(s1)
            .then_with(|| c2.times_seen.cmp(&c1.times_seen))
            .then_with(|| c2.last_seen_at.cmp(&c1.last_seen_at))
    });

    let results = scored
        .into_iter()
        .take(limit as usize)
        .map(|(_, c)| c)
        .collect();

    Ok(results)
}

/// List known contacts, most frequently used first.
pub fn list(db: &Db, limit: u64) -> Result<Vec<Contact>> {
    suggest(db, "", limit)
}

/// Remove one contact.
pub fn delete(db: &Db, address: &str) -> Result<()> {
    db.conn()
        .execute("delete from contacts where address = ?1", [address.trim()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seen_sets_transferred_name_as_initial_alias() {
        let db = Db::open_in_memory().unwrap();
        seen(&db, "alice@example.com", Some("Alice Smith")).unwrap();
        let s = suggest(&db, "alice", 5).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name.as_deref(), Some("Alice Smith"));
        assert_eq!(s[0].alias.as_deref(), Some("Alice Smith"));

        // User edits alias
        set_alias(&db, "alice@example.com", Some("Allie")).unwrap();
        let s2 = suggest(&db, "alice", 5).unwrap();
        assert_eq!(s2[0].alias.as_deref(), Some("Allie"));

        // New mail comes in with original name; user-customized alias survives
        seen(&db, "alice@example.com", Some("Alice Smith Jr.")).unwrap();
        let s3 = suggest(&db, "alice", 5).unwrap();
        assert_eq!(s3[0].name.as_deref(), Some("Alice Smith Jr."));
        assert_eq!(s3[0].alias.as_deref(), Some("Allie"));
        assert_eq!(s3[0].times_seen, 2);
    }

    #[test]
    fn fuzzy_matching_by_alias_and_domain() {
        let db = Db::open_in_memory().unwrap();
        seen(&db, "mail@obbt.de", Some("RR")).unwrap();
        seen(&db, "test@wiehl.lat", Some("Test")).unwrap();
        seen(&db, "amazon@renier.de", Some("Amazon")).unwrap();
        set_alias(&db, "mail@obbt.de", Some("My Boss")).unwrap();

        // Match by alias "Boss"
        let res = suggest(&db, "boss", 5).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].address, "mail@obbt.de");

        // Match by domain stem "wiehl"
        let res2 = suggest(&db, "wiehl", 5).unwrap();
        assert_eq!(res2.len(), 1);
        assert_eq!(res2[0].address, "test@wiehl.lat");

        // Match by domain "obbt.de"
        let res3 = suggest(&db, "obbt", 5).unwrap();
        assert_eq!(res3.len(), 1);
        assert_eq!(res3[0].address, "mail@obbt.de");

        // Fuzzy subsequence match "amz" -> amazon@renier.de
        let res4 = suggest(&db, "amz", 5).unwrap();
        assert_eq!(res4.len(), 1);
        assert_eq!(res4[0].address, "amazon@renier.de");
    }

    #[test]
    fn delete_and_list() {
        let db = Db::open_in_memory().unwrap();
        seen(&db, "alice@example.com", Some("Alice")).unwrap();
        seen(&db, "bob@example.com", None).unwrap();
        assert_eq!(list(&db, 10).unwrap().len(), 2);
        delete(&db, "bob@example.com").unwrap();
        assert_eq!(list(&db, 10).unwrap().len(), 1);
    }

    #[test]
    fn seed_from_messages_populates_transferred_names_and_aliases() {
        let db = Db::open_in_memory().unwrap();
        let aid = crate::store::accounts::create(
            &db,
            &crate::models::NewAccount {
                name: "Test".to_string(),
                email_address: "me@example.com".to_string(),
                from_name: String::new(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "me".to_string(),
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "me".to_string(),
                auth_vault_key: "vault".to_string(),
                check_interval_secs: 300,
            },
        )
        .unwrap();
        let fid =
            crate::store::folders::upsert(&db, aid, "INBOX", "/", crate::models::FolderRole::Inbox)
                .unwrap();

        let raw_hdr = "From: Bob Builder <bob@example.com>\r\nTo: Alice Wonderland <alice@example.com>\r\nSubject: Hi\r\n\r\n";
        crate::store::messages::upsert(
            &db,
            &crate::models::NewMessage {
                account_id: aid,
                folder_id: fid,
                uid: 1,
                message_id_header: None,
                thread_id: None,
                subject: Some("Hi".to_string()),
                from_addr: Some("bob@example.com".to_string()),
                to_addrs: vec!["alice@example.com".to_string()],
                cc_addrs: Vec::new(),
                bcc_addrs: Vec::new(),
                reply_to: None,
                date: Some("2026-01-01T00:00:00Z".to_string()),
                snippet: None,
                body_text: None,
                body_html: None,
                raw_headers: Some(raw_hdr.to_string()),
                is_read: true,
                is_starred: false,
                is_draft: false,
                has_attachments: false,
                keywords: Vec::new(),
                size: raw_hdr.len() as u64,
                downloaded_full: true,
            },
        )
        .unwrap();

        let seeded = seed_from_messages(&db).unwrap();
        assert!(seeded >= 2);

        let bob = suggest(&db, "builder", 5).unwrap();
        assert_eq!(bob.len(), 1);
        assert_eq!(bob[0].address, "bob@example.com");
        assert_eq!(bob[0].alias.as_deref(), Some("Bob Builder"));
        assert_eq!(bob[0].name.as_deref(), Some("Bob Builder"));

        let alice = suggest(&db, "wonderland", 5).unwrap();
        assert_eq!(alice.len(), 1);
        assert_eq!(alice[0].address, "alice@example.com");
        assert_eq!(alice[0].alias.as_deref(), Some("Alice Wonderland"));
    }
}
