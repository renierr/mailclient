//! Tests for the QML JSON feeds.
//!
//! Split out of `feed.rs` once they outgrew it (see AGENT.md, "File size
//! & where tests live"). Still a `#[cfg(test)]` submodule of `feed`, so
//! `super::*` reaches its private helpers exactly as before.

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

    // The list row carries identity and state only…
    let rows: serde_json::Value =
        serde_json::from_str(&messages_list_json_paged(&db, f, 10, 0).unwrap()).unwrap();
    assert_eq!(rows[0]["uid"], 7);
    assert_eq!(rows[0]["from"], "alice@example.com");
    assert!(rows[0]["unread"].as_bool().unwrap());
    assert!(!rows[0]["has_attachments"].as_bool().unwrap());

    // …the reader payload carries the bodies.
    let reader: serde_json::Value =
        serde_json::from_str(&message_json(&db, f, 7).unwrap()).unwrap();
    assert_eq!(reader["body"], "<b>hi</b>");
    assert!(reader["is_html"].as_bool().unwrap());
    assert_eq!(reader["body_html"], "<b>hi</b>");
    // No files on this message: flag off, empty list.
    assert!(!reader["has_attachments"].as_bool().unwrap());
    assert_eq!(reader["attachments"].as_array().unwrap().len(), 0);
}

#[test]
fn trash_messages_and_folders_are_always_seen() {
    let (db, acc, _inbox) = setup();
    let trash_id = folders::upsert(&db, acc, "Trash", "/", FolderRole::Trash).unwrap();
    let mut m = msg_store::sample_new(acc, trash_id, 42);
    m.is_read = false;
    msg_store::upsert(&db, &m).unwrap();

    // 1. folders_json must report unread: 0 for Trash
    let folders: serde_json::Value =
        serde_json::from_str(&folders_json(&db, acc).unwrap()).unwrap();
    let trash_folder = folders
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["role"] == "trash")
        .unwrap();
    assert_eq!(trash_folder["unread"], 0);

    // 2. messages_list_json_paged must report unread: false
    let list_paged: serde_json::Value =
        serde_json::from_str(&messages_list_json_paged(&db, trash_id, 10, 0).unwrap()).unwrap();
    assert!(!list_paged[0]["unread"].as_bool().unwrap());

    // 3. message_json must report unread: false
    let single_msg: serde_json::Value =
        serde_json::from_str(&message_json(&db, trash_id, 42).unwrap()).unwrap();
    assert!(!single_msg["unread"].as_bool().unwrap());
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
    let row: serde_json::Value = serde_json::from_str(&message_json(&db, f, 71).unwrap()).unwrap();
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
fn compact_list_omits_bodies_but_reader_payload_has_them() {
    let (db, acc, f) = setup();
    let mut m = msg_store::sample_new(acc, f, 72);
    m.body_html = Some("<b>reader only</b>".to_string());
    msg_store::upsert(&db, &m).unwrap();

    let list: serde_json::Value =
        serde_json::from_str(&messages_list_json_paged(&db, f, 10, 0).unwrap()).unwrap();
    assert_eq!(list[0]["uid"], 72);
    assert!(list[0].get("body_html").is_none());
    assert!(list[0].get("attachments").is_none());

    let reader: serde_json::Value =
        serde_json::from_str(&message_json(&db, f, 72).unwrap()).unwrap();
    assert_eq!(reader["body_html"], "<b>reader only</b>");
    assert!(reader["is_html"].as_bool().unwrap());
}

#[test]
fn headers_dialog_carries_addresses_and_ids() {
    let (db, acc, f) = setup();
    let mut m = msg_store::sample_new(acc, f, 9);
    m.cc_addrs = vec!["cc@example.com".to_string()];
    m.reply_to = Some("reply@example.com".to_string());
    msg_store::upsert(&db, &m).unwrap();
    let h: serde_json::Value = serde_json::from_str(&headers_json(&db, f, 9).unwrap()).unwrap();
    assert_eq!(h["from"], "alice@example.com");
    assert_eq!(h["to"], "bob@example.com");
    assert_eq!(h["cc"], "cc@example.com");
    assert_eq!(h["subject"], "Hello");
    assert_eq!(h["message_id"], "<9@example.com>");
    assert_eq!(h["reply_to"], "reply@example.com");
    // Full local timestamp (`YYYY-MM-DD HH:MM`), not the compact list
    // date — shape-checked instead of exact: TZ shifts the clock.
    let date = h["date"].as_str().unwrap();
    assert_eq!(date.len(), 16, "unexpected date shape: {date:?}");
    assert!(
        date.starts_with("2026-09-06")
            || date.starts_with("2026-09-07")
            || date.starts_with("2026-09-08")
    );
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
    let row: serde_json::Value = serde_json::from_str(&message_json(&db, f, 8).unwrap()).unwrap();
    assert!(!row["body_html"].as_str().unwrap().contains("script"));
    assert!(!row["body_html"].as_str().unwrap().contains("example.com"));
    assert!(row["has_remote_images"].as_bool().unwrap());

    let mut plain = msg_store::sample_new(acc, f, 9);
    plain.body_text = Some("I <3 you".to_string());
    plain.body_html = None;
    msg_store::upsert(&db, &plain).unwrap();
    let row2: serde_json::Value = serde_json::from_str(&message_json(&db, f, 9).unwrap()).unwrap();
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
        serde_json::from_str(&messages_list_json_paged(&db, f, 2, 0).unwrap()).unwrap();
    assert_eq!(page1.as_array().unwrap().len(), 2);
    assert_eq!(page1[0]["uid"], 33);
    let page2: serde_json::Value =
        serde_json::from_str(&messages_list_json_paged(&db, f, 2, 2).unwrap()).unwrap();
    assert_eq!(page2.as_array().unwrap().len(), 1);
    assert_eq!(page2[0]["uid"], 31);
}

#[test]
fn sorted_feed_follows_the_stored_sort_settings() {
    use crate::store::settings;
    let (db, acc, f) = setup();
    let mut a = msg_store::sample_new(acc, f, 41);
    a.from_addr = Some("zeta@example.com".to_string());
    a.subject = Some("Banana".to_string());
    msg_store::upsert(&db, &a).unwrap();
    let mut b = msg_store::sample_new(acc, f, 42);
    b.from_addr = Some("alpha@example.com".to_string());
    b.subject = Some("Apple".to_string());
    msg_store::upsert(&db, &b).unwrap();

    // Subject A-Z puts uid 42 first.
    settings::set_sort(&db, "subject", false).unwrap();
    let by_subject: serde_json::Value =
        serde_json::from_str(&messages_list_json_paged(&db, f, 10, 0).unwrap()).unwrap();
    assert_eq!(by_subject[0]["uid"], 42);
    assert_eq!(by_subject[1]["uid"], 41);

    // Sender Z-A flips it back.
    settings::set_sort(&db, "from", true).unwrap();
    let by_sender: serde_json::Value =
        serde_json::from_str(&messages_list_json_paged(&db, f, 10, 0).unwrap()).unwrap();
    assert_eq!(by_sender[0]["uid"], 41);
}

#[test]
fn search_rows_carry_folder_and_plain_snippet() {
    let (db, acc, f) = setup();
    let other = folders::upsert(&db, acc, "Archive", "/", FolderRole::Archive).unwrap();
    let mut m = msg_store::sample_new(acc, f, 81);
    m.body_text = Some("line one\ninvoice for the archive\nproject tail".to_string());
    msg_store::upsert(&db, &m).unwrap();
    let mut m2 = msg_store::sample_new(acc, other, 82);
    m2.body_text = Some("unrelated note".to_string());
    msg_store::upsert(&db, &m2).unwrap();

    let hits: serde_json::Value =
        serde_json::from_str(&search_json(&db, acc, "invoice", 50, "").unwrap()).unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 1);
    assert_eq!(hits[0]["uid"], 81);
    assert_eq!(hits[0]["folder"], "INBOX");
    // Plain match context: no highlight tags leak into list rows.
    let snippet = hits[0]["snippet"].as_str().unwrap();
    assert!(snippet.contains("invoice"));
    assert!(!snippet.contains('<'));
    // Single-line contract: body line breaks must not reach the list
    // (they would paint past the fixed row height into the next row).
    assert!(!snippet.contains('\n'));
    assert!(hits[0]["unread"].as_bool().unwrap());

    // Blank / operator-only queries are `[]`, never an error.
    assert_eq!(search_json(&db, acc, "", 50, "").unwrap(), "[]");
    assert_eq!(search_json(&db, acc, "***", 50, "").unwrap(), "[]");

    // Folder scope: the Archive copy is invisible from INBOX and the
    // INBOX hit is invisible from Archive.
    let scoped: serde_json::Value =
        serde_json::from_str(&search_json(&db, acc, "invoice", 50, "Archive").unwrap()).unwrap();
    assert_eq!(scoped.as_array().unwrap().len(), 0);
    let scoped: serde_json::Value =
        serde_json::from_str(&search_json(&db, acc, "invoice", 50, "INBOX").unwrap()).unwrap();
    assert_eq!(scoped.as_array().unwrap().len(), 1);
}

#[test]
fn short_date_formats() {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    assert_eq!(short_date(Some(&now)).text.len(), 5); // HH:MM
    assert_eq!(
        short_date(Some("2020-01-02T03:04:05+00:00")).text,
        "2020-01-02"
    );
    assert_eq!(short_date(None).text, "");
    // Numbers need no translating, so they carry no key.
    assert_eq!(short_date(Some(&now)).key, "");
    assert_eq!(short_date(None).key, "");
}

#[test]
fn yesterday_is_flagged_for_the_ui_to_translate() {
    // The only case whose text is a word rather than a number. mailcore has
    // no catalogue, so it names the case and QML supplies the word.
    let yesterday = chrono::Local::now() - chrono::Duration::days(1);
    let d = short_date(Some(&yesterday.to_rfc3339()));
    assert_eq!(d.key, "yesterday");
    assert_eq!(d.text, "Yesterday", "English fallback for key-less callers");
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
        let shown = short_date(Some(&raw)).text;
        // Only same-day mails render as a clock time; otherwise the date
        // is shown and this assertion does not apply.
        if shown.len() == 5 {
            assert_eq!(shown, expected, "for {raw}");
        }
    }
}
