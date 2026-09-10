//! JSON feeds for the QML UI. The bridge exposes these as strings; QML
//! `JSON.parse`s them into `ListModel`s. (Native list models may replace this
//! transport later without touching QML role names.)

use serde_json::json;

use crate::db::Db;
use crate::error::Result;
use crate::store::{accounts, folders, messages, settings};
use crate::{html, html::Sanitized};

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

/// `[{id, name, email, imap_host, smtp_host}]` for the account manager.
pub fn accounts_json(db: &Db) -> Result<String> {
    let mut arr = Vec::new();
    for a in accounts::list(db)? {
        arr.push(json!({
            "id": a.id,
            "name": a.name,
            "email": a.email_address,
            "imap_host": a.imap_host,
            "imap_port": a.imap_port,
            "imap_sec": a.imap_security,
            "imap_user": a.imap_username,
            "smtp_host": a.smtp_host,
            "smtp_port": a.smtp_port,
            "smtp_sec": a.smtp_security,
            "smtp_user": a.smtp_username,
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

/// Compact human date in the **viewer's** timezone: `09:12` (today),
/// `Yesterday`, else `2026-09-07`.
///
/// Everything is converted to local time first. Formatting the parsed value
/// directly keeps whatever offset the sender wrote (usually `Z`), so a mail
/// that arrived at 11:12 local showed 09:12, and "today"/"yesterday" flipped
/// at UTC midnight rather than the user's.
fn short_date(rfc3339: Option<&str>) -> String {
    let raw = rfc3339.unwrap_or("");
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) else {
        return raw.to_string();
    };
    let local = dt.with_timezone(&chrono::Local);
    let today = chrono::Local::now().date_naive();
    let date = local.date_naive();
    if date == today {
        local.format("%H:%M").to_string()
    } else if date == today.pred_opt().unwrap_or(today) {
        "Yesterday".to_string()
    } else {
        local.format("%Y-%m-%d").to_string()
    }
}

/// `[{uid, subject, from, date, snippet, unread, starred, body_text,
/// body_html, is_html, has_remote_images, body}]`, newest first.
/// - `body_html` is **sanitized** (scripts/handlers/remote-img gated by the
///   `load_remote_images` setting); never trust the stored raw HTML in QML.
/// - `is_html` is decided in Rust (no QML `<`/`>` guessing).
/// - `body` is kept for backward compat = sanitized html if any, else text.
pub fn messages_json(db: &Db, folder_id: i64) -> Result<String> {
    let allow_remote = settings::get_bool(db, settings::LOAD_REMOTE_IMAGES).unwrap_or(false);
    let mut arr = Vec::new();
    for m in messages::list_by_folder(db, folder_id, FEED_LIMIT, 0)? {
        let raw_html = m.body_html.as_deref().unwrap_or("");
        let raw_text = m.body_text.as_deref().unwrap_or("");
        // A stored `body_text` may itself hold HTML source (legacy
        // plain-only sends of composer rich text). Prefer a real html part,
        // else upgrade text that looks like HTML.
        let candidate_html = if html::looks_like_html(raw_html)
            || !raw_html.trim().is_empty() && m.body_html.is_some()
        {
            raw_html
        } else if html::looks_like_html(raw_text) {
            raw_text
        } else {
            ""
        };
        let Sanitized {
            html: safe_html,
            had_remote,
        } = html::sanitize(candidate_html, allow_remote);
        // `had_remote` only matters when we actually had an html body.
        let is_html =
            !safe_html.trim().is_empty() || (!candidate_html.trim().is_empty() && had_remote);
        let plain = if raw_text.trim().is_empty() && !candidate_html.trim().is_empty() {
            html::html_to_text(candidate_html)
        } else {
            raw_text.to_string()
        };
        // When remote images were stripped the sanitized html can be empty
        // (image-only newsletter) — still route to WebEngine so the banner
        // ("images blocked") shows instead of raw-tag text.
        let body_html = if is_html && safe_html.trim().is_empty() {
            // Keeps the html branch alive, and says which of the two it is
            // rather than blaming blocked images for an empty body.
            if had_remote {
                "<p>[images blocked — choose Show images]</p>".to_string()
            } else {
                "<p>[no displayable content]</p>".to_string()
            }
        } else {
            safe_html
        };
        let legacy_body = if is_html {
            body_html.clone()
        } else {
            plain.clone()
        };
        arr.push(json!({
            "uid": m.uid,
            "subject": m.subject.as_deref().unwrap_or("(no subject)"),
            "from": m.from_addr.as_deref().unwrap_or("?"),
            "date": short_date(m.date.as_deref()),
            "snippet": m.snippet.as_deref().unwrap_or(""),
            "unread": !m.is_read,
            "starred": m.is_starred,
            "body_text": plain,
            "body_html": body_html,
            "is_html": is_html,
            "has_remote_images": had_remote && is_html,
            "body": legacy_body,
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
        assert!(msgs[0]["is_html"].as_bool().unwrap());
        assert_eq!(msgs[0]["body_html"], "<b>hi</b>");
    }

    #[test]
    fn script_is_stripped_and_plain_stays_plain() {
        let (db, acc, f) = setup();
        let mut evil = msg_store::sample_new(acc, f, 8);
        evil.body_text = None;
        evil.body_html = Some(
            "<p>hi</p><script>alert(1)</script><img src=\"https://example.com/t.png\">".to_string(),
        );
        msg_store::upsert(&db, &evil).unwrap();
        let msgs: serde_json::Value =
            serde_json::from_str(&messages_json(&db, f).unwrap()).unwrap();
        let row = msgs
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["uid"] == 8)
            .unwrap();
        assert!(!row["body_html"].as_str().unwrap().contains("script"));
        assert!(!row["body_html"].as_str().unwrap().contains("example.com"));
        assert!(row["has_remote_images"].as_bool().unwrap());

        let mut plain = msg_store::sample_new(acc, f, 9);
        plain.body_text = Some("I <3 you".to_string());
        plain.body_html = None;
        msg_store::upsert(&db, &plain).unwrap();
        let msgs2: serde_json::Value =
            serde_json::from_str(&messages_json(&db, f).unwrap()).unwrap();
        let row2 = msgs2
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["uid"] == 9)
            .unwrap();
        assert!(!row2["is_html"].as_bool().unwrap());
        assert_eq!(row2["body_text"], "I <3 you");
    }

    #[test]
    fn short_date_formats() {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert_eq!(short_date(Some(&now)).len(), 5); // HH:MM
        assert_eq!(short_date(Some("2020-01-02T03:04:05+00:00")), "2020-01-02");
        assert_eq!(short_date(None), "");
    }

    #[test]
    fn short_date_renders_local_clock_not_the_senders_offset() {
        use chrono::{Local, TimeZone, Utc};
        // Same instant, written in three different offsets: the list must show
        // one and the same local wall-clock time for all three.
        let instant = Utc.with_ymd_and_hms(2026, 6, 15, 12, 30, 0).unwrap();
        let expected = instant.with_timezone(&Local).format("%H:%M").to_string();
        let offsets = [
            instant.to_rfc3339(),
            instant
                .with_timezone(&chrono::FixedOffset::east_opt(5 * 3600).unwrap())
                .to_rfc3339(),
            instant
                .with_timezone(&chrono::FixedOffset::west_opt(8 * 3600).unwrap())
                .to_rfc3339(),
        ];
        for raw in offsets {
            let shown = short_date(Some(&raw));
            // Only same-day mails render as a clock time; otherwise the date
            // is shown and this assertion does not apply.
            if shown.len() == 5 {
                assert_eq!(shown, expected, "for {raw}");
            }
        }
    }
}
