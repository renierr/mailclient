//! Full-text search over the FTS5 `messages_fts` index.

use rusqlite::params;

use crate::db::Db;
use crate::error::Result;

/// One search hit.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub message_id: i64,
    pub folder_id: i64,
    pub subject: Option<String>,
    pub from_addr: Option<String>,
    /// Highlighted snippet (`<b>…</b>` marks matches).
    pub snippet: String,
}

/// Turn free-text input into a safe FTS5 MATCH query.
///
/// Operators (`AND`/`OR`/`NEAR`/`"`/`*`) are stripped so a typed search
/// cannot change meaning or fail the MATCH. Tokens become prefix phrases.
#[must_use]
pub fn escape_fts_query(raw: &str) -> Option<String> {
    let mut terms = Vec::new();
    for token in raw.split_whitespace() {
        let cleaned: String = token
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '@' | '.'))
            .collect();
        if cleaned.is_empty() {
            continue;
        }
        terms.push(format!("\"{cleaned}\"*"));
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// Search subject/from/body of one account's messages.
pub fn search(db: &Db, account_id: i64, query: &str, limit: u64) -> Result<Vec<SearchHit>> {
    let Some(match_query) = escape_fts_query(query) else {
        return Ok(Vec::new());
    };
    let mut stmt = db.conn().prepare(
        "select m.id, m.folder_id, m.subject, m.from_addr,
                snippet(messages_fts, 2, '<b>', '</b>', '…', 12)
         from messages_fts
         join messages m on m.id = messages_fts.rowid
         where messages_fts match ?1 and m.account_id = ?2
         order by rank limit ?3",
    )?;
    let rows = stmt
        .query_map(params![match_query, account_id, limit as i64], |row| {
            Ok(SearchHit {
                message_id: row.get(0)?,
                folder_id: row.get(1)?,
                subject: row.get(2)?,
                from_addr: row.get(3)?,
                snippet: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount};
    use crate::store::{accounts, folders, messages};

    #[test]
    fn fts_finds_body_text() {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "a".to_string(),
                email_address: "a@x.y".to_string(),
                from_name: String::new(),
                imap_host: "h".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "h".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let f = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        let mut m = messages::sample_new(acc, f, 1);
        m.body_text = Some("the quick brown fox jumps".to_string());
        messages::upsert(&db, &m).unwrap();

        let hits = search(&db, acc, "quick", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("<b>quick</b>"));
        assert!(search(&db, acc, "zebra", 10).unwrap().is_empty());
    }

    #[test]
    fn fts_escapes_operators_and_quotes() {
        assert_eq!(escape_fts_query("quick"), Some("\"quick\"*".to_string()));
        assert_eq!(
            escape_fts_query("quick brown"),
            Some("\"quick\"* \"brown\"*".to_string())
        );
        assert_eq!(
            escape_fts_query("AND OR NEAR \"quoted\" foo*"),
            Some("\"AND\"* \"OR\"* \"NEAR\"* \"quoted\"* \"foo\"*".to_string())
        );
        assert_eq!(escape_fts_query("   \"\" ***   "), None);
        assert_eq!(
            escape_fts_query("user@example.com"),
            Some("\"user@example.com\"*".to_string())
        );
    }

    #[test]
    fn fts_operator_input_still_matches_plain_text() {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "a".to_string(),
                email_address: "a@x.y".to_string(),
                from_name: String::new(),
                imap_host: "h".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "h".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let f = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        let mut m = messages::sample_new(acc, f, 1);
        m.body_text = Some("quoted invoice AND notes".to_string());
        messages::upsert(&db, &m).unwrap();

        assert_eq!(search(&db, acc, "\"quoted\" AND", 10).unwrap().len(), 1);
        assert!(search(&db, acc, "\"\"", 10).unwrap().is_empty());
    }
}
