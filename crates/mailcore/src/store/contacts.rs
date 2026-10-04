//! `contacts` for address autocomplete with alias support and fuzzy search.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::Result;
use crate::models::Contact;
use crate::store::now;

/// Contacts only ever seen once are suggested for removal once they are this
/// old (compared against `last_seen_at`).
const STALE_AFTER_DAYS: i64 = 90;

/// Local parts that never belong to a human correspondent: no-reply senders,
/// bounce processors and mail-system accounts. Matching is deliberately
/// conservative (exact local part, or a `bounce*` prefix for VERP-style
/// bounce addresses) so real mailing lists are left alone.
const AUTOMATED_LOCAL_PARTS: &[&str] = &[
    "noreply",
    "no-reply",
    "no_reply",
    "donotreply",
    "do-not-reply",
    "do_not_reply",
    "dontreply",
    "do-not-respond",
    "postmaster",
    "mailer-daemon",
    "mailerdaemon",
    "mail-daemon",
    "maildaemon",
    "auto-reply",
    "autoreply",
];

/// True for automated senders (`noreply@…`, `mailer-daemon@…`, `bounce-*@…`)
/// that make noise in autocomplete and should never become contacts.
#[must_use]
pub fn is_automated_address(address: &str) -> bool {
    let local = address
        .trim()
        .split('@')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if local.is_empty() {
        return false;
    }
    AUTOMATED_LOCAL_PARTS.contains(&local.as_str()) || local.starts_with("bounce")
}

/// One contact the cleanup review suggests removing, with machine-readable
/// reasons (`"automated"`, `"stale"`) the frontends map to localized text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupCandidate {
    pub contact: Contact,
    pub reasons: Vec<String>,
}

/// Record having seen an address (insert or bump counter).
///
/// Seeds `alias` from `name` (the transferred real name) if `alias` is not
/// already populated. Automated senders (see [`is_automated_address`]) are
/// silently skipped so they never pollute autocomplete.
pub fn seen(db: &Db, address: &str, name: Option<&str>) -> Result<()> {
    let addr_clean = address.trim();
    if addr_clean.is_empty() || is_automated_address(addr_clean) {
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

/// Record having sent mail to an address: it counts as seen *and* as
/// sent-to (`sent_count`), which ranks the contact above merely harvested
/// ones and exempts it from the cleanup's `stale` reason. Automated
/// senders stay excluded even here.
pub fn seen_sent(db: &Db, address: &str, name: Option<&str>) -> Result<()> {
    let addr_clean = address.trim();
    if addr_clean.is_empty() || is_automated_address(addr_clean) {
        return Ok(());
    }
    let name_clean = name.map(str::trim).filter(|s| !s.is_empty());
    let ts = now();
    db.conn().execute(
        "insert into contacts (address, name, alias, times_seen, sent_count, last_seen_at)
         values (?1, ?2, ?2, 1, 1, ?3)
         on conflict (address) do update set
            name = coalesce(excluded.name, contacts.name),
            alias = coalesce(contacts.alias, excluded.name, contacts.name),
            times_seen = contacts.times_seen + 1,
            sent_count = contacts.sent_count + 1,
            last_seen_at = excluded.last_seen_at",
        params![addr_clean, name_clean, ts],
    )?;
    Ok(())
}

/// Backfill `sent_count` from cached Sent-folder mail (used by the v21
/// migration): every distinct recipient address per sent message counts
/// once. Only updates contacts that already exist — never resurrects
/// removed ones.
pub fn backfill_sent_counts_from_connection(conn: &rusqlite::Connection) -> Result<usize> {
    let mut stmt = conn.prepare(
        "select to_addrs, cc_addrs, bcc_addrs from messages
          join folders on messages.folder_id = folders.id
          where folders.role = 'sent'",
    )?;
    let mut counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let rows = stmt.query_map([], |row| {
        let to_addrs: String = row.get(0)?;
        let cc_addrs: String = row.get(1)?;
        let bcc_addrs: String = row.get(2)?;
        Ok((to_addrs, cc_addrs, bcc_addrs))
    })?;
    for row_res in rows {
        let (to_addrs, cc_addrs, bcc_addrs) = row_res?;
        let mut per_message = std::collections::HashSet::new();
        for raw in [&to_addrs, &cc_addrs, &bcc_addrs] {
            if let Ok(addrs) = serde_json::from_str::<Vec<String>>(raw) {
                for addr in addrs {
                    let clean = addr.trim();
                    if !clean.is_empty() && clean.contains('@') {
                        per_message.insert(clean.to_ascii_lowercase());
                    }
                }
            }
        }
        for addr in per_message {
            *counts.entry(addr).or_default() += 1;
        }
    }

    let mut credited = 0usize;
    for (addr, n) in &counts {
        credited += conn.execute(
            "update contacts set sent_count = sent_count + ?1 where lower(address) = ?2",
            rusqlite::params![n, addr],
        )?;
    }
    Ok(credited)
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
                                    if !email_clean.is_empty()
                                        && email_clean.contains('@')
                                        && !is_automated_address(email_clean)
                                    {
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
                if !clean.is_empty() && clean.contains('@') && !is_automated_address(clean) {
                    found.push((clean.to_string(), None, ts.clone()));
                }
            }
            if let Some(to_json) = to_addrs_json {
                if let Ok(addrs) = serde_json::from_str::<Vec<String>>(&to_json) {
                    for addr in addrs {
                        let clean = addr.trim();
                        if !clean.is_empty() && clean.contains('@') && !is_automated_address(clean)
                        {
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

    // People you wrote to outrank harvested ones: a single send counts as
    // much as five sightings.
    best_score.map(|s| {
        s + (contact.times_seen.min(50) as i64) * 2 + (contact.sent_count.min(20) as i64) * 10
    })
}

/// Top matches for `query` (matching alias, domain, name, or address),
/// ranked by fuzzy match quality and frequency.
pub fn suggest(db: &Db, query: &str, limit: u64) -> Result<Vec<Contact>> {
    let q = query.trim();
    if q.is_empty() {
        let mut stmt = db.conn().prepare(
            "select address, name, alias, times_seen, sent_count, last_seen_at from contacts
             order by sent_count desc, times_seen desc, last_seen_at desc limit ?1",
        )?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(Contact {
                    address: row.get(0)?,
                    name: row.get(1)?,
                    alias: row.get(2)?,
                    times_seen: row.get::<_, i64>(3)? as u64,
                    sent_count: row.get::<_, i64>(4)? as u64,
                    last_seen_at: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        return Ok(rows);
    }

    let mut stmt = db.conn().prepare(
        "select address, name, alias, times_seen, sent_count, last_seen_at from contacts",
    )?;
    let candidates = stmt
        .query_map([], |row| {
            Ok(Contact {
                address: row.get(0)?,
                name: row.get(1)?,
                alias: row.get(2)?,
                times_seen: row.get::<_, i64>(3)? as u64,
                sent_count: row.get::<_, i64>(4)? as u64,
                last_seen_at: row.get(5)?,
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

/// Remove several contacts at once (the cleanup review's multi-select).
/// Returns how many rows were removed.
pub fn delete_many(db: &Db, addresses: &[&str]) -> Result<u64> {
    if addresses.is_empty() {
        return Ok(0);
    }
    let tx_guard = db.conn();
    let mut removed = 0u64;
    for addr in addresses {
        let clean = addr.trim();
        if clean.is_empty() {
            continue;
        }
        removed += tx_guard.execute("delete from contacts where address = ?1", [clean])? as u64;
    }
    Ok(removed)
}

/// Contacts worth reviewing for removal: automated senders collected before
/// the [`seen`] filter existed, and one-off addresses not seen for a long
/// time. Each candidate carries machine-readable `reasons` (`"automated"`
/// and/or `"stale"`); the frontends decide the wording. Most recently seen
/// first, up to `limit`.
pub fn cleanup_candidates(db: &Db, limit: u64) -> Result<Vec<CleanupCandidate>> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(STALE_AFTER_DAYS);
    let mut stmt = db.conn().prepare(
        "select address, name, alias, times_seen, sent_count, last_seen_at from contacts
          order by last_seen_at desc",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Contact {
                address: row.get(0)?,
                name: row.get(1)?,
                alias: row.get(2)?,
                times_seen: row.get::<_, i64>(3)? as u64,
                sent_count: row.get::<_, i64>(4)? as u64,
                last_seen_at: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut out = Vec::new();
    for contact in rows {
        if out.len() >= limit as usize {
            break;
        }
        let mut reasons = Vec::new();
        if is_automated_address(&contact.address) {
            reasons.push("automated".to_string());
        }
        // Someone you wrote to is never stale, however long ago.
        let stale = contact.sent_count == 0
            && contact.times_seen <= 1
            && chrono::DateTime::parse_from_rfc3339(&contact.last_seen_at)
                .map(|seen| seen < cutoff)
                .unwrap_or(false);
        if stale {
            reasons.push("stale".to_string());
        }
        if !reasons.is_empty() {
            out.push(CleanupCandidate { contact, reasons });
        }
    }
    Ok(out)
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
    fn automated_senders_are_never_stored() {
        let db = Db::open_in_memory().unwrap();
        for addr in [
            "noreply@example.com",
            "no-reply@example.com",
            "NO_REPLY@example.com",
            "donotreply@example.com",
            "do-not-reply@example.com",
            "postmaster@example.com",
            "mailer-daemon@example.com",
            "bounce-123@bounces.example.com",
            "  Noreply@example.com  ",
        ] {
            seen(&db, addr, Some("Some Name")).unwrap();
        }
        // Real people and real lists still pass.
        seen(&db, "alice@example.com", Some("Alice")).unwrap();
        seen(&db, "team-news@example.com", Some("Team News")).unwrap();
        seen(&db, "owner-announce@example.com", None).unwrap();
        let all = list(&db, 20).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn cleanup_candidates_flags_automated_and_stale() {
        let db = Db::open_in_memory().unwrap();
        // Automated row as collected before the filter existed.
        db.conn()
            .execute(
                "insert into contacts (address, name, alias, times_seen, last_seen_at)
                 values ('noreply@example.com', null, null, 5, '2026-10-04T00:00:00Z')",
                [],
            )
            .unwrap();
        // One-off address last seen long ago.
        db.conn()
            .execute(
                "insert into contacts (address, name, alias, times_seen, last_seen_at)
                 values ('once@example.com', null, null, 1, '2020-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
        // Healthy rows: seen often, or seen once but recently.
        seen(&db, "friend@example.com", Some("Friend")).unwrap();
        seen(&db, "friend@example.com", Some("Friend")).unwrap();
        seen(&db, "new@example.com", None).unwrap();

        let cands = cleanup_candidates(&db, 50).unwrap();
        assert_eq!(cands.len(), 2);
        let noreply = cands
            .iter()
            .find(|c| c.contact.address == "noreply@example.com")
            .unwrap();
        assert_eq!(noreply.reasons, vec!["automated".to_string()]);
        let once = cands
            .iter()
            .find(|c| c.contact.address == "once@example.com")
            .unwrap();
        assert_eq!(once.reasons, vec!["stale".to_string()]);
    }

    #[test]
    fn sent_addresses_outrank_merely_seen_ones() {
        let db = Db::open_in_memory().unwrap();
        for _ in 0..5 {
            seen(&db, "stranger@example.com", None).unwrap();
        }
        seen_sent(&db, "friend@example.com", Some("Friend")).unwrap();

        let all = list(&db, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].address, "friend@example.com");
        assert_eq!(all[0].sent_count, 1);
        assert_eq!(all[1].sent_count, 0);

        // The autocomplete agrees: same domain match, the sent contact wins.
        let res = suggest(&db, "example", 10).unwrap();
        assert_eq!(res[0].address, "friend@example.com");
    }

    #[test]
    fn sent_contacts_are_never_stale_but_automated_stays_excluded() {
        let db = Db::open_in_memory().unwrap();
        // One send long ago: backdate the row like an old database would.
        seen_sent(&db, "old-friend@example.com", None).unwrap();
        db.conn()
            .execute(
                "update contacts set last_seen_at = '2020-01-01T00:00:00Z'
                 where address = 'old-friend@example.com'",
                [],
            )
            .unwrap();
        // Even a deliberate send must not create automated contacts.
        seen_sent(&db, "noreply@example.com", None).unwrap();

        let cands = cleanup_candidates(&db, 50).unwrap();
        assert!(cands
            .iter()
            .all(|c| c.contact.address != "old-friend@example.com"));
        assert_eq!(list(&db, 10).unwrap().len(), 1);
    }

    #[test]
    fn backfill_credits_sent_mail_without_resurrecting_anyone() {
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
        let sent =
            crate::store::folders::upsert(&db, aid, "Sent", "/", crate::models::FolderRole::Sent)
                .unwrap();
        let inbox =
            crate::store::folders::upsert(&db, aid, "INBOX", "/", crate::models::FolderRole::Inbox)
                .unwrap();

        // A known contact and an unknown one, both mailed twice.
        seen(&db, "friend@example.com", Some("Friend")).unwrap();
        for uid in [1u32, 2] {
            crate::store::messages::upsert(
                &db,
                &crate::models::NewMessage {
                    account_id: aid,
                    folder_id: sent,
                    uid,
                    message_id_header: None,
                    thread_id: None,
                    subject: None,
                    from_addr: Some("me@example.com".to_string()),
                    from_name: None,
                    to_addrs: vec![
                        "friend@example.com".to_string(),
                        "ghost@example.com".to_string(),
                    ],
                    cc_addrs: Vec::new(),
                    bcc_addrs: Vec::new(),
                    reply_to: None,
                    date: Some("2026-01-01T00:00:00Z".to_string()),
                    snippet: None,
                    body_text: None,
                    body_html: None,
                    raw_headers: None,
                    is_read: true,
                    is_starred: false,
                    is_draft: false,
                    has_attachments: false,
                    keywords: Vec::new(),
                    size: 0,
                    downloaded_full: true,
                },
            )
            .unwrap();
        }
        // Same address from the inbox must not count as sent.
        crate::store::messages::upsert(
            &db,
            &crate::models::NewMessage {
                account_id: aid,
                folder_id: inbox,
                uid: 1,
                message_id_header: None,
                thread_id: None,
                subject: None,
                from_addr: Some("friend@example.com".to_string()),
                from_name: None,
                to_addrs: vec!["me@example.com".to_string()],
                cc_addrs: Vec::new(),
                bcc_addrs: Vec::new(),
                reply_to: None,
                date: Some("2026-01-01T00:00:00Z".to_string()),
                snippet: None,
                body_text: None,
                body_html: None,
                raw_headers: None,
                is_read: true,
                is_starred: false,
                is_draft: false,
                has_attachments: false,
                keywords: Vec::new(),
                size: 0,
                downloaded_full: true,
            },
        )
        .unwrap();

        let credited = backfill_sent_counts_from_connection(db.conn()).unwrap();
        assert_eq!(credited, 1);
        let friend = suggest(&db, "friend", 5).unwrap();
        assert_eq!(friend.len(), 1);
        assert_eq!(friend[0].sent_count, 2);
        // The unknown recipient is not resurrected into contacts.
        assert!(suggest(&db, "ghost", 5).unwrap().is_empty());
    }

    #[test]
    fn delete_many_removes_selection() {
        let db = Db::open_in_memory().unwrap();
        seen(&db, "a@example.com", None).unwrap();
        seen(&db, "b@example.com", None).unwrap();
        seen(&db, "c@example.com", None).unwrap();
        assert_eq!(
            delete_many(&db, &["a@example.com", "b@example.com"]).unwrap(),
            2
        );
        assert_eq!(delete_many(&db, &[]).unwrap(), 0);
        let rest = list(&db, 10).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].address, "c@example.com");
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
                from_name: Some("Bob Builder".to_string()),
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
