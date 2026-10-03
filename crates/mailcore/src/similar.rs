//! Find similar messages to a given message across the account.
//!
//! Matches in three tiers:
//! 1. Same conversation thread (`thread_id`).
//! 2. Same sender and normalized subject (strip Re:/Fwd:, case-insensitive).
//! 3. Subject keywords via the existing FTS5 index (`messages_fts`), best
//!    match first.
//!
//! Results exclude the target message itself and reuse the exact same JSON
//! structure as [`crate::feed::search_json`]: grouped by folder, in tier order
//! inside each folder.

use std::collections::HashSet;

use rusqlite::{params, OptionalExtension};

use crate::db::Db;
use crate::error::Result;
use crate::feed::{hit_json, hits_json, HitRow};

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

/// `s` without a leading ASCII-case-insensitive `prefix`. Never slices inside
/// a multi-byte character.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

/// Strip reply/forward prefixes, brackets and whitespace, returning a lowercase
/// canonical subject for grouping discussions.
#[must_use]
pub fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let prev = s;
        // Strip bracketed tag prefixes like [Re], [Fwd] or [Fwd: ...]
        if let Some(rest) = s.strip_prefix('[') {
            if let Some(end) = rest.find(']') {
                let inside = rest[..end].trim();
                if PREFIXES
                    .iter()
                    .any(|p| inside.eq_ignore_ascii_case(p.trim_end_matches(':')))
                {
                    s = rest[end + 1..].trim_start();
                    continue;
                }
                if let Some(remainder) = PREFIXES
                    .iter()
                    .find_map(|p| strip_prefix_ci(rest.trim_start(), p))
                {
                    let remainder = remainder.trim_start();
                    s = match remainder.strip_suffix(']') {
                        Some(r) => r.trim(),
                        None => remainder,
                    };
                    continue;
                }
            }
        }
        if let Some(rest) = PREFIXES.iter().find_map(|p| strip_prefix_ci(s, p)) {
            s = rest.trim_start();
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
        if clean.chars().count() >= 3
            && !STOP_WORDS.contains(&clean.as_str())
            && !keywords.contains(&clean)
        {
            keywords.push(clean);
            if keywords.len() >= 5 {
                break;
            }
        }
    }
    keywords
}

/// FTS5 query matching any keyword in the subject column.
fn keyword_query(keywords: &[String]) -> Option<String> {
    if keywords.is_empty() {
        return None;
    }
    let terms = keywords
        .iter()
        .map(|k| format!("\"{}\"", k.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ");
    Some(format!("{{subject}} : ({terms})"))
}

/// Retrieve the subject of a target message (for displaying the search chip).
pub fn target_subject(db: &Db, account_id: i64, folder_id: i64, uid: i64) -> Result<String> {
    let subject: Option<String> = db
        .conn()
        .query_row(
            "select subject from messages
              where account_id = ?1 and folder_id = ?2 and uid = ?3",
            params![account_id, folder_id, uid],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(subject
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no subject)".to_string()))
}

/// Columns every tier selects, in the order [`match_row`] reads them. The
/// snippet column differs (FTS tiers use `snippet()`), so it is spliced in.
fn select_columns(snippet: &str) -> String {
    format!(
        "m.id, m.uid, m.folder_id, f.path, m.subject, m.from_addr, m.date,
         {snippet}, m.is_read, m.is_starred, m.has_attachments, m.from_name"
    )
}

struct MatchRow {
    id: i64,
    hit: HitRow,
}

fn match_row(r: &rusqlite::Row) -> rusqlite::Result<MatchRow> {
    Ok(MatchRow {
        id: r.get(0)?,
        hit: HitRow {
            uid: r.get(1)?,
            folder_id: r.get(2)?,
            folder: r.get(3)?,
            subject: r.get(4)?,
            from_addr: r.get(5)?,
            date: r.get(6)?,
            snippet: r.get(7)?,
            is_read: r.get::<_, i64>(8)? != 0,
            is_starred: r.get::<_, i64>(9)? != 0,
            has_attachments: r.get::<_, i64>(10)? != 0,
            from_name: r.get(11)?,
        },
    })
}

/// Accumulates matches across tiers: skips the target and repeats, stops at
/// the limit.
struct Collector {
    seen: HashSet<i64>,
    rows: Vec<MatchRow>,
    limit: usize,
}

impl Collector {
    fn full(&self) -> bool {
        self.rows.len() >= self.limit
    }

    fn remaining(&self) -> i64 {
        self.limit.saturating_sub(self.rows.len()) as i64
    }

    fn push(&mut self, row: MatchRow) {
        if !self.full() && self.seen.insert(row.id) {
            self.rows.push(row);
        }
    }
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
    let conn = db.conn();
    let target = conn
        .query_row(
            "select id, thread_id, subject, from_addr
               from messages
              where account_id = ?1 and folder_id = ?2 and uid = ?3",
            params![account_id, folder_id, uid],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                ))
            },
        )
        .optional()?;
    let Some((target_id, target_thread_id, target_subject, target_from)) = target else {
        return Ok("[]".to_string());
    };

    let target_norm = normalize_subject(&target_subject);
    let mut found = Collector {
        seen: HashSet::from([target_id]),
        rows: Vec::new(),
        limit: limit.max(1) as usize,
    };
    let plain = select_columns("m.snippet");

    // Tier 1: same thread.
    if !target_thread_id.is_empty() {
        let mut stmt = conn.prepare(&format!(
            "select {plain}
               from messages m
               join folders f on f.id = m.folder_id
              where m.account_id = ?1
                and m.thread_id = ?2
                and m.id not in (select message_id from pending_moves)
              order by m.date desc, m.id desc
              limit ?3"
        ))?;
        // +1: the target sits in its own thread and is skipped.
        let rows = stmt.query_map(
            params![account_id, target_thread_id, found.remaining() + 1],
            match_row,
        )?;
        for row in rows {
            found.push(row?);
        }
    }

    // Tier 2: same sender, same normalized subject. Normalizing happens in
    // Rust, so scan the sender's subjects (cheap columns only) and fetch full
    // rows for the matches.
    if !found.full() && !target_from.is_empty() && !target_norm.is_empty() {
        let ids: Vec<i64> = {
            let mut stmt = conn.prepare(
                "select m.id, m.subject
                   from messages m
                  where m.account_id = ?1
                    and m.from_addr = ?2
                    and m.id not in (select message_id from pending_moves)
                  order by m.date desc, m.id desc",
            )?;
            let mut ids = Vec::new();
            let mut rows = stmt.query(params![account_id, target_from])?;
            while let Some(r) = rows.next()? {
                let id: i64 = r.get(0)?;
                let subject: Option<String> = r.get(1)?;
                if !found.seen.contains(&id)
                    && normalize_subject(subject.as_deref().unwrap_or("")) == target_norm
                {
                    ids.push(id);
                    if ids.len() as i64 >= found.remaining() {
                        break;
                    }
                }
            }
            ids
        };
        let mut stmt = conn.prepare(&format!(
            "select {plain}
               from messages m
               join folders f on f.id = m.folder_id
              where m.id = ?1"
        ))?;
        for id in ids {
            found.push(stmt.query_row(params![id], match_row)?);
        }
    }

    // Tier 3: subject keywords, best FTS rank first.
    if !found.full() {
        if let Some(match_query) = keyword_query(&extract_keywords(&target_norm)) {
            let mut stmt = conn.prepare(&format!(
                "select {}
                   from messages_fts
                   join messages m on m.id = messages_fts.rowid
                   join folders f on f.id = m.folder_id
                  where messages_fts match ?1
                    and m.account_id = ?2
                    and m.id not in (select message_id from pending_moves)
                  order by messages_fts.rank, m.date desc, m.id desc
                  limit ?3",
                select_columns("snippet(messages_fts, 6, '', '', '…', 12)")
            ))?;
            // +seen: rows already taken by earlier tiers come back too.
            let limit = found.remaining() + found.seen.len() as i64;
            let rows = stmt.query_map(params![match_query, account_id, limit], match_row)?;
            for row in rows {
                found.push(row?);
            }
        }
    }

    hits_json(found.rows.into_iter().map(|r| hit_json(r.hit)).collect())
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
    fn normalize_subject_survives_multibyte_subjects() {
        // Fixed-width prefix slicing used to panic inside these characters.
        assert_eq!(normalize_subject("Привет"), "привет");
        assert_eq!(normalize_subject("日本語"), "日本語");
        assert_eq!(normalize_subject("Re: Öäü…"), "öäü…");
        assert_eq!(normalize_subject("[日本] 語"), "[日本] 語");
        assert_eq!(normalize_subject("[Ré: x]"), "[ré: x]");
        assert_eq!(normalize_subject("AW: Grüße"), "grüße");
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

    fn add(db: &Db, acc: i64, folder: i64, uid: u32, subject: &str, from: &str) {
        let mut m = messages::sample_new(acc, folder, uid);
        m.thread_id = None;
        m.subject = Some(subject.into());
        m.from_addr = Some(from.into());
        messages::upsert(db, &m).unwrap();
    }

    fn uids(json: &str) -> Vec<u64> {
        let hits: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
        hits.iter().map(|h| h["uid"].as_u64().unwrap()).collect()
    }

    #[test]
    fn similar_handles_non_ascii_subjects_from_the_same_sender() {
        let (db, acc, folder) = setup_test_db();
        add(&db, acc, folder, 1, "Привет мир", "a@example.com");
        add(&db, acc, folder, 2, "日本語", "a@example.com");
        add(&db, acc, folder, 3, "Re: Привет мир", "a@example.com");
        assert_eq!(
            uids(&similar_json(&db, acc, folder, 1, 50).unwrap()),
            vec![3]
        );
    }

    #[test]
    fn keyword_tier_matches_subjects_only_and_ranks_best_first() {
        let (db, acc, folder) = setup_test_db();
        add(
            &db,
            acc,
            folder,
            1,
            "Quarterly budget review",
            "a@example.com",
        );
        add(&db, acc, folder, 2, "Budget", "b@example.com");
        add(
            &db,
            acc,
            folder,
            3,
            "Quarterly budget review notes",
            "c@example.com",
        );
        // Body mentions the keywords, subject does not: no match.
        let mut body_only = messages::sample_new(acc, folder, 4);
        body_only.thread_id = None;
        body_only.subject = Some("Lunch".into());
        body_only.body_text = Some("quarterly budget review".into());
        messages::upsert(&db, &body_only).unwrap();

        let hits = uids(&similar_json(&db, acc, folder, 1, 50).unwrap());
        assert_eq!(hits, vec![3, 2]);
    }

    #[test]
    fn similar_stops_at_the_limit() {
        let (db, acc, folder) = setup_test_db();
        add(&db, acc, folder, 1, "Weekly report", "a@example.com");
        for uid in 2..10 {
            add(&db, acc, folder, uid, "Re: Weekly report", "a@example.com");
        }
        assert_eq!(
            uids(&similar_json(&db, acc, folder, 1, 3).unwrap()).len(),
            3
        );
    }

    #[test]
    fn target_subject_falls_back_for_missing_messages() {
        let (db, acc, folder) = setup_test_db();
        assert_eq!(
            target_subject(&db, acc, folder, 999).unwrap(),
            "(no subject)"
        );
    }
}
