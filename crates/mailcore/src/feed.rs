//! JSON feeds for the QML UI. The bridge exposes these as strings; QML
//! `JSON.parse`s them into `ListModel`s. (Native list models may replace this
//! transport later without touching QML role names.)

use serde_json::json;

use crate::db::Db;
use crate::error::Result;
use crate::store::{folders, messages};

/// Max messages per folder feed (keeps QML lists snappy).
pub const FEED_LIMIT: u64 = 200;

/// `[{name, role, unread}]` ordered by path.
pub fn folders_json(db: &Db, account_id: i64) -> Result<String> {
    let mut arr = Vec::new();
    for f in folders::list_by_account(db, account_id)? {
        arr.push(json!({
            "id": f.id,
            "name": f.path,
            "role": f.role.as_str(),
            "unread": messages::count_unread(db, f.id)?,
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

/// Compact human date: `09:12` (today), `Yesterday`, else `2026-09-07`.
fn short_date(rfc3339: Option<&str>) -> String {
    let raw = rfc3339.unwrap_or("");
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) else {
        return raw.to_string();
    };
    let date = dt.date_naive();
    let today = chrono::Utc::now().date_naive();
    if date == today {
        dt.format("%H:%M").to_string()
    } else if date == today.pred_opt().unwrap_or(today) {
        "Yesterday".to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

/// `[{uid, subject, from, date, snippet, unread, starred, body}]`, newest first.
pub fn messages_json(db: &Db, folder_id: i64) -> Result<String> {
    let mut arr = Vec::new();
    for m in messages::list_by_folder(db, folder_id, FEED_LIMIT, 0)? {
        arr.push(json!({
            "uid": m.uid,
            "subject": m.subject.as_deref().unwrap_or("(no subject)"),
            "from": m.from_addr.as_deref().unwrap_or("?"),
            "date": short_date(m.date.as_deref()),
            "snippet": m.snippet.as_deref().unwrap_or(""),
            "unread": !m.is_read,
            "starred": m.is_starred,
            "body": m.body_html.as_deref().or(m.body_text.as_deref()).unwrap_or(""),
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount};
    use crate::store::{accounts, messages as msg_store};

    fn setup() -> (Db, i64, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "a".to_string(),
                email_address: "a@example.com".to_string(),
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
        (db, acc, f)
    }

    #[test]
    fn feeds_shape_matches_qml_roles() {
        let (db, acc, f) = setup();
        let mut m = msg_store::sample_new(acc, f, 7);
        m.body_html = Some("<b>hi</b>".to_string());
        msg_store::upsert(&db, &m).unwrap();

        let folders: serde_json::Value =
            serde_json::from_str(&folders_json(&db, acc).unwrap()).unwrap();
        assert_eq!(folders[0]["name"], "INBOX");
        assert_eq!(folders[0]["role"], "inbox");
        assert_eq!(folders[0]["unread"], 1);

        let msgs: serde_json::Value =
            serde_json::from_str(&messages_json(&db, f).unwrap()).unwrap();
        assert_eq!(msgs[0]["uid"], 7);
        assert_eq!(msgs[0]["from"], "alice@example.com");
        assert!(msgs[0]["unread"].as_bool().unwrap());
        assert_eq!(msgs[0]["body"], "<b>hi</b>");
    }

    #[test]
    fn short_date_formats() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert_eq!(short_date(Some(&now)).len(), 5); // HH:MM
        assert_eq!(short_date(Some("2020-01-02T03:04:05+00:00")), "2020-01-02");
        assert_eq!(short_date(None), "");
    }
}
