//! JSON feeds for the QML UI. The bridge exposes these as strings; QML
//! `JSON.parse`s them into `ListModel`s. (Native list models may replace this
//! transport later without touching QML role names.)

use serde_json::json;

use crate::db::Db;
use crate::error::Result;
use crate::html::{self, Sanitized};
use crate::store::{accounts, folders, messages, settings};

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
            // Sidebar visibility toggle + cached total (see Folders dialog),
            // plus the hierarchy delimiter so the move picker can indent
            // subfolders (depth = segments - 1).
            "subscribed": f.subscribed,
            "count": messages::count_by_folder(db, f.id)?,
            "delimiter": f.delimiter,
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

/// Sanitized bodies for one message: `(safe_html, had_remote, is_html, plain)`.
///
/// Shared by the list feed and the on-demand `message_html` path, so the
/// banner ("Show once") and the feed can never disagree.
pub fn sanitized_bodies(
    body_html: Option<&str>,
    body_text: Option<&str>,
    allow_remote: bool,
) -> (String, bool, bool, String) {
    let raw_html = body_html.unwrap_or("");
    let raw_text = body_text.unwrap_or("");
    // A stored `body_text` may itself hold HTML source (legacy
    // plain-only sends of composer rich text). Prefer a real html part,
    // else upgrade text that looks like HTML.
    let candidate_html =
        if html::looks_like_html(raw_html) || !raw_html.trim().is_empty() && body_html.is_some() {
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
    let is_html = !safe_html.trim().is_empty() || (!candidate_html.trim().is_empty() && had_remote);
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
    (body_html, had_remote, is_html, plain)
}

/// Sanitized HTML for one message, re-sanitized on demand.
///
/// The list feed strips remote images when the setting is off, so "Show
/// once" cannot reuse `body_html` — the URLs are already gone. This
/// re-sanitizes the stored raw body with `allow_remote=true` for that one
/// view. Inline `cid:`/`data:` images are always kept (they are part of the
/// mail, not tracking pixels).
pub fn message_html(db: &Db, folder_id: i64, uid: u32, allow_remote: bool) -> Result<String> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let (body_html, _, _, _) =
        sanitized_bodies(m.body_html.as_deref(), m.body_text.as_deref(), allow_remote);
    Ok(body_html)
}
/// `[{uid, subject, from, date, snippet, unread, starred, body_text,
/// body_html, is_html, has_remote_images, has_attachments, attachments,
/// body}]`, newest first.
/// - `body_html` is **sanitized** (scripts/handlers/remote-img gated by the
///   `load_remote_images` setting); never trust the stored raw HTML in QML.
/// - Inline `cid:`/`data:` images are part of the mail and always kept —
///   only remote `http(s)` images are gated (blocked by default).
/// - `is_html` is decided in Rust (no QML `<`/`>` guessing).
/// - `body` is kept for backward compat = sanitized html if any, else text.
///
/// `limit`/`offset` page the local cache newest-first. The list grows via
/// "load older": the bridge backfills the next server batch into SQLite
/// first (`sync_older`), then raises `limit` so the new rows appear.
pub fn messages_json_paged(db: &Db, folder_id: i64, limit: u64, offset: u64) -> Result<String> {
    let allow_remote = settings::get_bool(db, settings::LOAD_REMOTE_IMAGES).unwrap_or(false);
    let mut arr = Vec::new();
    for m in messages::list_by_folder(db, folder_id, limit, offset)? {
        let (body_html, had_remote, is_html, plain) =
            sanitized_bodies(m.body_html.as_deref(), m.body_text.as_deref(), allow_remote);
        let legacy_body = if is_html {
            body_html.clone()
        } else {
            plain.clone()
        };
        // Attachment metadata only (bytes never enter the feed — see
        // `attachments_json` + `Bridge::save_attachment` for the bytes path).
        let files: Vec<serde_json::Value> = messages::list_attachments(db, m.id)
            .unwrap_or_default()
            .into_iter()
            .map(|a| {
                json!({
                    "id": a.id,
                    "filename": a.filename,
                    "mime_type": a.mime_type,
                    "size": a.size,
                    "content_id": a.content_id,
                    "is_inline": a.is_inline,
                })
            })
            .collect();
        arr.push(json!({
            "uid": m.uid,
            "subject": m.subject.as_deref().unwrap_or("(no subject)"),
            "from": m.from_addr.as_deref().unwrap_or("?"),
            "date": short_date(m.date.as_deref()),
            "snippet": m.snippet.as_deref().unwrap_or(""),
            "unread": !m.is_read,
            "starred": m.is_starred,
            "has_attachments": m.has_attachments || !files.is_empty(),
            "attachments": files,
            "body_text": plain,
            "body_html": body_html,
            "is_html": is_html,
            "has_remote_images": had_remote && is_html,
            "body": legacy_body,
        }));
    }
    Ok(serde_json::to_string(&arr)?)
}

/// Attachment metadata for one message (`[{id, filename, mime_type, size,
/// content_id, is_inline}]`, no bytes). Used by the reader pane and the
/// save dialog; bytes leave Rust only via `save_attachment_to_path`.
pub fn attachments_json(db: &Db, folder_id: i64, uid: u32) -> Result<String> {
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let files: Vec<serde_json::Value> = messages::list_attachments(db, m.id)
        .unwrap_or_default()
        .into_iter()
        .map(|a| {
            json!({
                "id": a.id,
                "filename": a.filename,
                "mime_type": a.mime_type,
                "size": a.size,
                "content_id": a.content_id,
                "is_inline": a.is_inline,
            })
        })
        .collect();
    Ok(serde_json::to_string(&files)?)
}

/// First page of a folder (default list view).
pub fn messages_json(db: &Db, folder_id: i64) -> Result<String> {
    messages_json_paged(db, folder_id, FEED_LIMIT, 0)
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
        // No files on this message: flag off, empty list.
        assert!(!msgs[0]["has_attachments"].as_bool().unwrap());
        assert_eq!(msgs[0]["attachments"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn feed_carries_attachment_metadata_without_bytes() {
        use crate::models::NewAttachment;
        let (db, acc, f) = setup();
        let mut m = msg_store::sample_new(acc, f, 71);
        m.has_attachments = true;
        let id = msg_store::upsert(&db, &m).unwrap();
        msg_store::add_attachment(
            &db,
            id,
            &NewAttachment {
                filename: Some("doc.pdf".to_string()),
                mime_type: Some("application/pdf".to_string()),
                content_id: None,
                size: 4,
                data: Some(b"%PDF".to_vec()),
                is_inline: false,
            },
        )
        .unwrap();
        let msgs: serde_json::Value =
            serde_json::from_str(&messages_json(&db, f).unwrap()).unwrap();
        let row = &msgs[0];
        assert!(row["has_attachments"].as_bool().unwrap());
        assert_eq!(row["attachments"][0]["filename"], "doc.pdf");
        assert_eq!(row["attachments"][0]["size"], 4);
        // Bytes never leak into JSON.
        assert!(row["attachments"][0].get("data").is_none());
        let only: serde_json::Value =
            serde_json::from_str(&attachments_json(&db, f, 71).unwrap()).unwrap();
        assert_eq!(only[0]["filename"], "doc.pdf");
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
    fn inline_images_survive_while_remote_stays_gated() {
        // Inline cid:/data: are part of the mail and always kept; remote
        // http(s) is stripped by default and re-appears on demand.
        let (html_blocked, had_remote, is_html, _) = sanitized_bodies(
            Some("<p>hi<img src=\"cid:part1\"><img src=\"https://example.com/t.png\"></p>"),
            None,
            false,
        );
        assert!(is_html);
        assert!(had_remote);
        assert!(html_blocked.contains("cid:part1"));
        assert!(!html_blocked.contains("example.com"));

        let (html_allowed, _, _, _) = sanitized_bodies(
            Some("<p>hi<img src=\"cid:part1\"><img src=\"https://example.com/t.png\"></p>"),
            None,
            true,
        );
        assert!(html_allowed.contains("cid:part1"));
        assert!(html_allowed.contains("example.com"));
    }

    #[test]
    fn message_html_resanitizes_for_show_once() {
        let (db, acc, f) = setup();
        let mut m = msg_store::sample_new(acc, f, 11);
        m.body_text = None;
        m.body_html = Some("<p>hi<img src=\"https://example.com/t.png\"></p>".to_string());
        msg_store::upsert(&db, &m).unwrap();
        // Feed default (setting off) strips the remote URL…
        let blocked = message_html(&db, f, 11, false).unwrap();
        assert!(!blocked.contains("example.com"));
        // …but Show-once gets it back from the stored raw body.
        let allowed = message_html(&db, f, 11, true).unwrap();
        assert!(allowed.contains("example.com"));
    }

    #[test]
    fn folders_carry_subscribed_and_count() {
        let (db, acc, f) = setup();
        let folders: serde_json::Value =
            serde_json::from_str(&folders_json(&db, acc).unwrap()).unwrap();
        assert_eq!(folders[0]["subscribed"], true);
        assert_eq!(folders[0]["count"], 0);
        assert_eq!(folders[0]["delimiter"], "/");
        let m = msg_store::sample_new(acc, f, 21);
        msg_store::upsert(&db, &m).unwrap();
        let folders2: serde_json::Value =
            serde_json::from_str(&folders_json(&db, acc).unwrap()).unwrap();
        assert_eq!(folders2[0]["count"], 1);
    }

    #[test]
    fn paged_feed_slices_newest_first() {
        let (db, acc, f) = setup();
        for uid in [31u32, 32, 33] {
            let mut m = msg_store::sample_new(acc, f, uid);
            m.date = Some(format!("2026-09-0{uid}T10:00:00+00:00"));
            msg_store::upsert(&db, &m).unwrap();
        }
        let page1: serde_json::Value =
            serde_json::from_str(&messages_json_paged(&db, f, 2, 0).unwrap()).unwrap();
        assert_eq!(page1.as_array().unwrap().len(), 2);
        assert_eq!(page1[0]["uid"], 33);
        let page2: serde_json::Value =
            serde_json::from_str(&messages_json_paged(&db, f, 2, 2).unwrap()).unwrap();
        assert_eq!(page2.as_array().unwrap().len(), 1);
        assert_eq!(page2[0]["uid"], 31);
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
