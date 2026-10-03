//! Find similar messages to a given message across the account.
//!
//! Matches in three tiers:
//! 1. Same conversation thread (`thread_id`).
//! 2. Same sender and normalized subject (strip Re:/Fwd:, case-insensitive).
//! 3. Subject content keywords via the existing FTS5 index (`messages_fts`).
//!
//! Results exclude the target message itself and reuse the exact same JSON
//! structure as [`crate::feed::search_json`] (grouped by folder, newest first).

use std::collections::HashSet;

use rusqlite::params;
use serde_json::json;

use crate::badge::sender_badge;
use crate::db::Db;
use crate::error::Result;
use crate::feed::{one_line, short_date};

/// English, German, French common stop words that carry little topic signal.
const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "that", "this", "your", "have", "are", "was", "will",
    "what", "when", "here", "there", "about", "der", "die", "das", "und", "von", "mit", "für",
    "fuer", "auf", "ein", "eine", "einer", "nicht", "sich", "dem", "den", "des", "les", "pour",
    "avec", "dans", "sur", "une", "est",
];

/// Known reply and forward prefixes to strip when normalizing subjects.
const PREFIXES: &[&str] = &[
    "re:", "fwd:", "fw:", "aw:", "wg:", "sv:", "vs:", "antw:", "tr:",
];

/// Strip reply/forward prefixes, brackets and whitespace, returning a lowercase
/// canonical subject for grouping discussions.
#[must_use]
pub fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let prev = s;
        // Strip bracketed tag prefixes like [Re], [Fwd] or [Fwd: ...]
        if s.starts_with('[') {
            if let Some(end) = s.find(']') {
                let inside = s[1..end].trim();
                if PREFIXES
                    .iter()
                    .any(|p| inside.eq_ignore_ascii_case(p.trim_end_matches(':')))
                {
                    s = s[end + 1..].trim_start();
                    continue;
                }
                if let Some(matching) = PREFIXES
                    .iter()
                    .find(|p| inside.len() >= p.len() && inside[..p.len()].eq_ignore_ascii_case(p))
                {
                    let remainder = s[1 + matching.len()..].trim_start();
                    if let Some(r) = remainder.strip_suffix(']') {
                        s = r.trim();
                    } else {
                        s = remainder;
                    }
                    continue;
                }
            }
        }
        for prefix in PREFIXES {
            if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
                s = s[prefix.len()..].trim_start();
                break;
            }
        }
        if s == prev {
            break;
        }
    }
    s.to_lowercase()
}

/// Extract up to 5 meaningful keyword tokens from the subject for FTS searching.
#[must_use]
pub fn extract_keywords(normalized: &str) -> Vec<String> {
    let mut keywords = Vec::new();
    for word in normalized.split_whitespace() {
        let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
        if clean.len() >= 3 && !STOP_WORDS.contains(&clean.as_str()) {
            keywords.push(clean);
            if keywords.len() >= 5 {
                break;
            }
        }
    }
    keywords
}

/// Retrieve the subject of a target message (for displaying the search chip).
pub fn target_subject(db: &Db, account_id: i64, folder_id: i64, uid: i64) -> Result<String> {
    let mut stmt = db.conn().prepare(
        "select subject from messages
          where account_id = ?1 and folder_id = ?2 and uid = ?3",
    )?;
    let subject: Option<String> = stmt
        .query_row(params![account_id, folder_id, uid], |r| r.get(0))
        .unwrap_or(None);
    Ok(subject
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no subject)".to_string()))
}

struct MatchRow {
    uid: u32,
    folder_id: i64,
    folder_path: String,
    subject: Option<String>,
    from_addr: Option<String>,
    from_name: Option<String>,
    date: Option<String>,
    snippet: String,
    is_read: bool,
    is_starred: bool,
    has_attachments: bool,
}

/// Query similar messages across the account, formatted as JSON matching
/// [`crate::feed::search_json`]. Excludes the target message itself.
pub fn similar_json(
    db: &Db,
    account_id: i64,
    folder_id: i64,
    uid: i64,
    limit: u64,
) -> Result<String> {
    // 1. Locate target message
    let target = {
        let mut stmt = db.conn().prepare(
            "select id, thread_id, subject, from_addr
              from messages
              where account_id = ?1 and folder_id = ?2 and uid = ?3",
        )?;
        let mut rows = stmt.query(params![account_id, folder_id, uid])?;
        if let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let thread_id: Option<String> = row.get(1)?;
            let subject: Option<String> = row.get(2)?;
            let from_addr: Option<String> = row.get(3)?;
            Some((
                id,
                thread_id.unwrap_or_default(),
                subject.unwrap_or_default(),
                from_addr.unwrap_or_default(),
            ))
        } else {
            None
        }
    };

    let Some((target_id, target_thread_id, target_subject, target_from)) = target else {
        return Ok("[]".to_string());
    };

    let target_norm = normalize_subject(&target_subject);
    let keywords = extract_keywords(&target_norm);

    let mut seen_ids: HashSet<i64> = HashSet::new();
    seen_ids.insert(target_id);

    let mut collected: Vec<MatchRow> = Vec::new();
    let max_limit = limit.max(1) as usize;

    // --- Tier 1: Same thread_id ---------------------------------------------
    if !target_thread_id.is_empty() {
        let mut stmt = db.conn().prepare(
            "select m.id, m.uid, m.folder_id, f.path, m.subject, m.from_addr, m.date,
                    m.snippet, m.is_read, m.is_starred, m.has_attachments, m.from_name
               from messages m
               join folders f on f.id = m.folder_id
              where m.account_id = ?1
                and m.id != ?2
                and m.thread_id is not null
                and m.thread_id = ?3
                and m.id not in (select message_id from pending_moves)
              order by m.date desc, m.id desc
              limit ?4",
        )?;
        let rows = stmt.query_map(
            params![account_id, target_id, target_thread_id, limit as i64],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    MatchRow {
                        uid: r.get::<_, u32>(1)?,
                        folder_id: r.get::<_, i64>(2)?,
                        folder_path: r.get::<_, String>(3)?,
                        subject: r.get::<_, Option<String>>(4)?,
                        from_addr: r.get::<_, Option<String>>(5)?,
                        date: r.get::<_, Option<String>>(6)?,
                        snippet: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                        is_read: r.get::<_, i64>(8)? != 0,
                        is_starred: r.get::<_, i64>(9)? != 0,
                        has_attachments: r.get::<_, i64>(10)? != 0,
                        from_name: r.get::<_, Option<String>>(11)?,
                    },
                ))
            },
        )?;
        for row in rows {
            let (id, match_row) = row?;
            if seen_ids.insert(id) {
                collected.push(match_row);
            }
        }
    }

    // --- Tier 2: Same sender + normalized subject ---------------------------
    if collected.len() < max_limit && !target_from.is_empty() && !target_norm.is_empty() {
        let remaining = (max_limit - collected.len()) as i64;
        let mut stmt = db.conn().prepare(
            "select m.id, m.uid, m.folder_id, f.path, m.subject, m.from_addr, m.date,
                    m.snippet, m.is_read, m.is_starred, m.has_attachments, m.from_name
               from messages m
               join folders f on f.id = m.folder_id
              where m.account_id = ?1
                and m.id != ?2
                and m.from_addr is not null
                and m.from_addr = ?3
                and m.id not in (select message_id from pending_moves)
              order by m.date desc, m.id desc
              limit ?4",
        )?;
        let rows = stmt.query_map(
            params![account_id, target_id, target_from, remaining * 2],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    MatchRow {
                        uid: r.get::<_, u32>(1)?,
                        folder_id: r.get::<_, i64>(2)?,
                        folder_path: r.get::<_, String>(3)?,
                        subject: r.get::<_, Option<String>>(4)?,
                        from_addr: r.get::<_, Option<String>>(5)?,
                        date: r.get::<_, Option<String>>(6)?,
                        snippet: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                        is_read: r.get::<_, i64>(8)? != 0,
                        is_starred: r.get::<_, i64>(9)? != 0,
                        has_attachments: r.get::<_, i64>(10)? != 0,
                        from_name: r.get::<_, Option<String>>(11)?,
                    },
                ))
            },
        )?;
        for row in rows {
            let (id, match_row) = row?;
            if !seen_ids.contains(&id) {
                let subj = match_row.subject.as_deref().unwrap_or("");
                if normalize_subject(subj) == target_norm && seen_ids.insert(id) {
                    collected.push(match_row);
                    if collected.len() >= max_limit {
                        break;
                    }
                }
            }
        }
    }

    // --- Tier 3: Subject keywords in FTS5 ------------------------------------
    if collected.len() < max_limit && !keywords.is_empty() {
        let fts_input = keywords.join(" ");
        if let Some(match_query) = crate::search::fts_query(&fts_input) {
            let remaining = (max_limit - collected.len()) as i64;
            let mut stmt = db.conn().prepare(
                "select m.id, m.uid, m.folder_id, f.path, m.subject, m.from_addr, m.date,
                        snippet(messages_fts, 6, '', '', '…', 12),
                        m.is_read, m.is_starred, m.has_attachments, m.from_name
                   from messages_fts
                   join messages m on m.id = messages_fts.rowid
                   join folders f on f.id = m.folder_id
                  where messages_fts match ?1
                    and m.account_id = ?2
                    and m.id != ?3
                    and m.id not in (select message_id from pending_moves)
                  order by m.date desc, m.id desc
                  limit ?4",
            )?;
            let rows = stmt.query_map(
                params![match_query, account_id, target_id, remaining],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        MatchRow {
                            uid: r.get::<_, u32>(1)?,
                            folder_id: r.get::<_, i64>(2)?,
                            folder_path: r.get::<_, String>(3)?,
                            subject: r.get::<_, Option<String>>(4)?,
                            from_addr: r.get::<_, Option<String>>(5)?,
                            date: r.get::<_, Option<String>>(6)?,
                            snippet: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                            is_read: r.get::<_, i64>(8)? != 0,
                            is_starred: r.get::<_, i64>(9)? != 0,
                            has_attachments: r.get::<_, i64>(10)? != 0,
                            from_name: r.get::<_, Option<String>>(11)?,
                        },
                    ))
                },
            )?;
            for row in rows {
                let (id, match_row) = row?;
                if seen_ids.insert(id) {
                    collected.push(match_row);
                    if collected.len() >= max_limit {
                        break;
                    }
                }
            }
        }
    }

    // Build JSON rows formatted identically to search_json
    let mut arr = Vec::with_capacity(collected.len());
    for row in collected {
        let date = short_date(row.date.as_deref());
        let from = row.from_addr.unwrap_or_else(|| "?".to_string());
        let from_name = row.from_name.unwrap_or_default();
        let badge = sender_badge(&from_name, &from);
        let mut hit = json!({
            "uid": row.uid,
            "folder_id": row.folder_id,
            "folder": row.folder_path,
            "subject": row.subject.unwrap_or_else(|| "(no subject)".to_string()),
            "from": from,
            "from_name": from_name,
            "date": date.text,
            "date_key": date.key,
            "snippet": one_line(&row.snippet),
            "unread": !row.is_read,
            "starred": row.is_starred,
            "has_attachments": row.has_attachments,
        });
        badge.extend(&mut hit);
        arr.push(hit);
    }

    // Stable grouping: folders grouped in order of their first/highest hit
    let mut order: Vec<String> = Vec::new();
    for hit in &arr {
        let f = hit["folder"].as_str().unwrap_or_default();
        if !order.iter().any(|o| o == f) {
            order.push(f.to_string());
        }
    }
    arr.sort_by_key(|hit| {
        let f = hit["folder"].as_str().unwrap_or_default();
        order.iter().position(|o| o == f)
    });

    Ok(serde_json::to_string(&arr)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount};
    use crate::store::{accounts, folders, messages};

    #[test]
    fn normalize_subject_strips_nested_prefixes_and_brackets() {
        assert_eq!(normalize_subject("Hello"), "hello");
        assert_eq!(normalize_subject("Re: Hello"), "hello");
        assert_eq!(normalize_subject("RE:  re: Hello World"), "hello world");
        assert_eq!(
            normalize_subject("Fwd: Re: [Fwd: Invoice #123]"),
            "invoice #123"
        );
        assert_eq!(normalize_subject("AW: WG: Status update"), "status update");
        assert_eq!(
            normalize_subject("[Re] Meeting tomorrow"),
            "meeting tomorrow"
        );
    }

    #[test]
    fn extract_keywords_filters_stop_words_and_short_words() {
        let kw = extract_keywords("important invoice for the monthly billing and accounting");
        assert_eq!(
            kw,
            vec!["important", "invoice", "monthly", "billing", "accounting"]
        );

        let kw2 = extract_keywords("hi is a re");
        assert!(kw2.is_empty());
    }

    fn setup_test_db() -> (Db, i64, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc_id = accounts::create(
            &db,
            &NewAccount {
                name: "Test".into(),
                email_address: "test@example.com".into(),
                from_name: "Tester".into(),
                imap_host: "h".into(),
                imap_port: 993,
                imap_security: "tls".into(),
                imap_username: "u".into(),
                smtp_host: "h".into(),
                smtp_port: 465,
                smtp_security: "tls".into(),
                smtp_username: "u".into(),
                auth_vault_key: "k".into(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let folder_id = folders::upsert(&db, acc_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        (db, acc_id, folder_id)
    }

    #[test]
    fn similar_finds_thread_and_subject_and_excludes_target() {
        let (db, acc_id, folder_id) = setup_test_db();

        // Target message
        let mut m1 = messages::sample_new(acc_id, folder_id, 101);
        m1.thread_id = Some("thread-xyz".into());
        m1.subject = Some("Project Roadmap Discussion".into());
        m1.from_addr = Some("alice@example.com".into());
        messages::upsert(&db, &m1).unwrap();

        // Message 2: Same thread, different subject
        let mut m2 = messages::sample_new(acc_id, folder_id, 102);
        m2.thread_id = Some("thread-xyz".into());
        m2.subject = Some("Re: Changed Topic".into());
        m2.from_addr = Some("bob@example.com".into());
        messages::upsert(&db, &m2).unwrap();

        // Message 3: Same sender + normalized subject, no thread_id
        let mut m3 = messages::sample_new(acc_id, folder_id, 103);
        m3.thread_id = None;
        m3.subject = Some("Re: Project Roadmap Discussion".into());
        m3.from_addr = Some("alice@example.com".into());
        messages::upsert(&db, &m3).unwrap();

        // Target subject
        let subj = target_subject(&db, acc_id, folder_id, 101).unwrap();
        assert_eq!(subj, "Project Roadmap Discussion");

        // Similar query
        let res_json = similar_json(&db, acc_id, folder_id, 101, 50).unwrap();
        let hits: Vec<serde_json::Value> = serde_json::from_str(&res_json).unwrap();

        // Target (UID 101) must NOT be in the results
        assert!(!hits.iter().any(|h| h["uid"] == 101));

        // Both 102 (thread) and 103 (sender + subject) must be present
        assert_eq!(hits.len(), 2);
        let uids: Vec<u64> = hits.iter().map(|h| h["uid"].as_u64().unwrap()).collect();
        assert!(uids.contains(&102));
        assert!(uids.contains(&103));
    }
}
