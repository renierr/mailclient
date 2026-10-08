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
    assert_eq!(rows[0]["from_name"], "Alice");
    assert!(rows[0]["unread"].as_bool().unwrap());
    assert!(!rows[0]["has_attachments"].as_bool().unwrap());
    // Raw UTC timestamp for the list date quick-filter (`search::date_passes`).
    assert!(rows[0].get("date_raw").is_some());

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
fn list_row_without_a_sender_name_falls_back_to_the_address() {
    let (db, acc, f) = setup();
    let mut m = msg_store::sample_new(acc, f, 7);
    m.from_name = None;
    msg_store::upsert(&db, &m).unwrap();

    let rows: serde_json::Value =
        serde_json::from_str(&messages_list_json_paged(&db, f, 10, 0).unwrap()).unwrap();
    assert_eq!(rows[0]["from"], "alice@example.com");
    assert_eq!(rows[0]["from_name"], "");
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
    msg_store::add_attachment(
        &db,
        id,
        &NewAttachment {
            filename: Some("../con".to_string()),
            mime_type: None,
            content_id: None,
            size: 1,
            data: None,
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
    // What to show and what a written file is called come from the core.
    assert_eq!(only[0]["display_name"], "doc.pdf");
    assert_eq!(only[0]["file_name"], "doc.pdf");
    assert_eq!(only[1]["display_name"], "../con");
    assert_eq!(only[1]["file_name"], "_con");
    // Preformatted sizes ride along, so both readers show one text.
    assert_eq!(only[0]["size_text"], "4 B");
}

#[test]
fn reader_hides_cid_shown_images_predating_the_inline_flag() {
    use crate::models::NewAttachment;
    let (db, acc, f) = setup();
    let mut m = msg_store::sample_new(acc, f, 72);
    m.body_html = Some("<p><img src=\"cid:yellowLogo\"></p>".to_string());
    // Stored before the `cid:` rule: disposition attachment, flag raised.
    m.has_attachments = true;
    let id = msg_store::upsert(&db, &m).unwrap();
    msg_store::add_attachment(
        &db,
        id,
        &NewAttachment {
            filename: Some("inline".to_string()),
            mime_type: Some("image/png".to_string()),
            content_id: Some("yellowLogo".to_string()),
            size: 8,
            data: Some(b"\x89PNG\r\n\x1a\nxx".to_vec()),
            is_inline: false,
        },
    )
    .unwrap();
    let row: serde_json::Value = serde_json::from_str(&message_json(&db, f, 72).unwrap()).unwrap();
    assert!(!row["has_attachments"].as_bool().unwrap());
    assert_eq!(row["attachments"].as_array().unwrap().len(), 0);
    let only: serde_json::Value =
        serde_json::from_str(&attachments_json(&db, f, 72).unwrap()).unwrap();
    assert_eq!(only.as_array().unwrap().len(), 0);
}

#[test]
fn extensionless_files_gain_theirs_from_the_mime_type() {
    use crate::models::NewAttachment;
    let (db, acc, f) = setup();
    let mut m = msg_store::sample_new(acc, f, 73);
    m.has_attachments = true;
    let id = msg_store::upsert(&db, &m).unwrap();
    msg_store::add_attachment(
        &db,
        id,
        &NewAttachment {
            filename: Some("inline".to_string()),
            mime_type: Some("image/png".to_string()),
            content_id: None,
            size: 8,
            data: None,
            is_inline: false,
        },
    )
    .unwrap();
    let aid = msg_store::add_attachment(
        &db,
        id,
        &NewAttachment {
            filename: None,
            mime_type: Some("application/pdf".to_string()),
            content_id: None,
            size: 4,
            data: None,
            is_inline: false,
        },
    )
    .unwrap();
    let only: serde_json::Value =
        serde_json::from_str(&attachments_json(&db, f, 73).unwrap()).unwrap();
    assert_eq!(only[0]["file_name"], "inline.png");
    assert_eq!(
        only[1]["file_name"],
        format!("attachment-{aid}.pdf").as_str()
    );
    assert_eq!(
        only[1]["display_name"],
        format!("attachment-{aid}.pdf").as_str()
    );
}

#[test]
fn every_sender_row_carries_one_badge() {
    let (db, acc, f) = setup();
    msg_store::upsert(&db, &msg_store::sample_new(acc, f, 9)).unwrap();
    let want = crate::badge::sender_badge("Alice", "alice@example.com");

    let rows: serde_json::Value =
        serde_json::from_str(&messages_list_json_paged(&db, f, 10, 0).unwrap()).unwrap();
    let reader: serde_json::Value =
        serde_json::from_str(&message_json(&db, f, 9).unwrap()).unwrap();
    for row in [&rows[0], &reader] {
        assert_eq!(row["initials"], want.initials.as_str());
        assert_eq!(row["avatar_light"], want.avatar_light.as_str());
        assert_eq!(row["avatar_dark"], want.avatar_dark.as_str());
    }
    assert_eq!(reader["from_name"], "Alice");
    assert_eq!(reader["reply_target"], "alice@example.com");
    assert_eq!(reader["reply_to_differs"], false);

    let accounts: serde_json::Value = serde_json::from_str(&accounts_json(&db).unwrap()).unwrap();
    assert_eq!(accounts[0]["initials"].as_str().unwrap().len(), 2);
    assert!(accounts[0]["avatar_dark"]
        .as_str()
        .unwrap()
        .starts_with('#'));
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
fn folders_carry_depth_and_leaf_for_indent_and_label() {
    let (db, acc, _inbox) = setup();
    folders::upsert(&db, acc, "Work/Client", "/", FolderRole::Custom).unwrap();
    let folders: serde_json::Value =
        serde_json::from_str(&folders_json(&db, acc).unwrap()).unwrap();
    let inbox = folders
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "INBOX")
        .unwrap();
    assert_eq!(inbox["depth"], 0);
    assert_eq!(inbox["leaf"], "INBOX");
    let sub = folders
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Work/Client")
        .unwrap();
    assert_eq!(sub["depth"], 1);
    assert_eq!(sub["leaf"], "Client");
    assert_eq!(folder_depth("a.b.c", "."), 2);
    assert_eq!(folder_leaf("a.b.c", "."), "c");
    assert_eq!(folder_depth("INBOX", ""), 0);
    assert_eq!(folder_leaf("INBOX", ""), "INBOX");
}

#[test]
fn folders_carry_always_visible_for_collapse() {
    let (db, acc, _inbox) = setup();
    folders::upsert(&db, acc, "INBOX/Archive", "/", FolderRole::Archive).unwrap();
    folders::upsert(&db, acc, "INBOX/Work", "/", FolderRole::Custom).unwrap();
    folders::upsert(&db, acc, "Work/Client", "/", FolderRole::Custom).unwrap();
    let folders: serde_json::Value =
        serde_json::from_str(&folders_json(&db, acc).unwrap()).unwrap();
    let by_name = |name: &str| {
        folders
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name)
            .unwrap()
            .clone()
    };
    // Known folders stay visible even nested below another folder, and so
    // do the inbox's direct children (servers that file everything below
    // the inbox); only deeper custom subfolders hide inside a collapsed
    // parent.
    assert_eq!(by_name("INBOX")["always_visible"], true);
    assert_eq!(by_name("INBOX/Archive")["always_visible"], true);
    assert_eq!(by_name("INBOX/Work")["always_visible"], true);
    assert_eq!(by_name("Work/Client")["always_visible"], false);
    assert_eq!(parent_path("Work/Client", "/").as_deref(), Some("Work"));
    assert_eq!(parent_path("INBOX", "/"), None);
    assert_eq!(parent_path("a.b.c", ".").as_deref(), Some("a.b"));
    assert_eq!(parent_path("INBOX", ""), None);
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
    // A hit wears the same badge as its list row.
    let rows: serde_json::Value =
        serde_json::from_str(&messages_list_json_paged(&db, f, 10, 0).unwrap()).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["uid"] == 81)
        .unwrap();
    assert_eq!(hits[0]["avatar_dark"], row["avatar_dark"]);
    assert_eq!(hits[0]["initials"], row["initials"]);
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

fn hit_uids(db: &Db, acc: i64, query: &str) -> Vec<u32> {
    let hits: Vec<serde_json::Value> =
        serde_json::from_str(&search_json(db, acc, query, 50, "").unwrap()).unwrap();
    let mut uids: Vec<u32> = hits
        .iter()
        .map(|h| h["uid"].as_u64().unwrap() as u32)
        .collect();
    uids.sort_unstable();
    uids
}

#[test]
fn search_reads_names_recipients_phrases_and_fields() {
    let (db, acc, f) = setup();
    let mut a = msg_store::sample_new(acc, f, 1);
    a.from_name = Some("Anna Example".to_string());
    a.from_addr = Some("anna@example.com".to_string());
    a.to_addrs = vec!["team@example.org".to_string()];
    a.subject = Some("Project plan".to_string());
    a.body_text = Some("the plan for the project".to_string());
    msg_store::upsert(&db, &a).unwrap();
    let mut b = msg_store::sample_new(acc, f, 2);
    b.from_name = Some("Bert".to_string());
    b.from_addr = Some("bert@example.com".to_string());
    b.cc_addrs = vec!["anna@example.com".to_string()];
    b.subject = Some("Reminder".to_string());
    b.body_text = Some("project plan reminder".to_string());
    msg_store::upsert(&db, &b).unwrap();

    // The sender's display name is indexed, not only the address.
    assert_eq!(hit_uids(&db, acc, "Anna"), [1, 2]);
    assert_eq!(hit_uids(&db, acc, "from:anna"), [1]);
    // `to:` covers Cc as well.
    assert_eq!(hit_uids(&db, acc, "to:anna"), [2]);
    assert_eq!(hit_uids(&db, acc, "to:team"), [1]);
    // A phrase keeps its word order; loose words do not.
    assert_eq!(hit_uids(&db, acc, r#""project plan""#), [1, 2]);
    assert_eq!(hit_uids(&db, acc, r#""plan for""#), [1]);
    assert_eq!(hit_uids(&db, acc, "plan project"), [1, 2]);
    assert_eq!(hit_uids(&db, acc, "subject:reminder"), [2]);
    assert_eq!(hit_uids(&db, acc, "project -reminder"), [1]);
    assert_eq!(
        hit_uids(&db, acc, "project -from:anna -to:anna"),
        Vec::<u32>::new()
    );
    // Exclusions alone search nothing.
    assert_eq!(hit_uids(&db, acc, "-reminder"), Vec::<u32>::new());
    // Typed operators are text, never syntax errors.
    assert_eq!(
        hit_uids(&db, acc, r#"project AND NOT ( "#),
        Vec::<u32>::new()
    );
    assert_eq!(hit_uids(&db, acc, "pro* NEAR("), Vec::<u32>::new());
}

#[test]
fn search_rows_are_newest_first_grouped_by_folder() {
    let (db, acc, f) = setup();
    let other = folders::upsert(&db, acc, "Archive", "/", FolderRole::Archive).unwrap();
    // Relevance would rank the subject-and-body hit first; date order wins,
    // and each folder's hits stay together behind its newest one.
    for (folder, uid, date, body) in [
        (f, 91, "2026-01-01T10:00:00Z", "invoice invoice invoice"),
        (other, 92, "2026-03-01T10:00:00Z", "one invoice"),
        (f, 93, "2026-02-01T10:00:00Z", "another invoice"),
        (other, 94, "2026-01-15T10:00:00Z", "older invoice"),
    ] {
        let mut m = msg_store::sample_new(acc, folder, uid);
        m.date = Some(date.to_string());
        m.body_text = Some(body.to_string());
        msg_store::upsert(&db, &m).unwrap();
    }
    let hits: serde_json::Value =
        serde_json::from_str(&search_json(&db, acc, "invoice", 50, "").unwrap()).unwrap();
    let uids: Vec<_> = hits
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["uid"].clone())
        .collect();
    assert_eq!(uids, [92, 94, 93, 91]);
}

#[test]
fn search_with_filter_tokens() {
    let (db, acc, f) = setup();

    // 101: unread, starred, has_attachments, 2026-08-15
    let mut m1 = msg_store::sample_new(acc, f, 101);
    m1.subject = Some("Invoice for August".to_string());
    m1.is_read = false;
    m1.is_starred = true;
    m1.has_attachments = true;
    m1.date = Some("2026-08-15T10:00:00Z".to_string());
    msg_store::upsert(&db, &m1).unwrap();

    // 102: read, unstarred, no attachments, 2026-09-10
    let mut m2 = msg_store::sample_new(acc, f, 102);
    m2.subject = Some("Invoice for September".to_string());
    m2.is_read = true;
    m2.is_starred = false;
    m2.has_attachments = false;
    m2.date = Some("2026-09-10T10:00:00Z".to_string());
    msg_store::upsert(&db, &m2).unwrap();

    // 103: unread, unstarred, no attachments, 2026-09-20
    let mut m3 = msg_store::sample_new(acc, f, 103);
    m3.subject = Some("Newsletter".to_string());
    m3.is_read = false;
    m3.is_starred = false;
    m3.has_attachments = false;
    m3.date = Some("2026-09-20T10:00:00Z".to_string());
    msg_store::upsert(&db, &m3).unwrap();

    assert_eq!(hit_uids(&db, acc, "is:unread"), [101, 103]);
    assert_eq!(hit_uids(&db, acc, "is:read"), [102]);
    assert_eq!(hit_uids(&db, acc, "is:starred"), [101]);
    assert_eq!(hit_uids(&db, acc, "is:unstarred"), [102, 103]);
    assert_eq!(hit_uids(&db, acc, "has:attachment"), [101]);
    assert_eq!(hit_uids(&db, acc, "after:2026-09-01"), [102, 103]);
    assert_eq!(hit_uids(&db, acc, "before:2026-09-01"), [101]);
    assert_eq!(hit_uids(&db, acc, "invoice is:unread"), [101]);
    assert_eq!(hit_uids(&db, acc, "invoice is:read"), [102]);
    assert_eq!(hit_uids(&db, acc, "is:unread after:2026-09-01"), [103]);
    // Lenient date spellings are normalised before the text comparison.
    assert_eq!(hit_uids(&db, acc, "after:2026-9-1"), [102, 103]);
}

#[test]
fn filter_only_search_honours_exclusions() {
    let (db, acc, f) = setup();
    for (uid, subject, from, read) in [
        (111, "Newsletter weekly", "news@example.com", false),
        (112, "Invoice", "bob@example.com", false),
        (113, "Newsletter monthly", "news@example.com", true),
        (114, "Meeting", "anna@example.com", false),
    ] {
        let mut m = msg_store::sample_new(acc, f, uid);
        m.subject = Some(subject.to_string());
        m.from_addr = Some(from.to_string());
        m.is_read = read;
        msg_store::upsert(&db, &m).unwrap();
    }
    assert_eq!(hit_uids(&db, acc, "is:unread"), [111, 112, 114]);
    assert_eq!(hit_uids(&db, acc, "-newsletter is:unread"), [112, 114]);
    assert_eq!(hit_uids(&db, acc, "is:unread -newsletter -from:bob"), [114]);
    assert_eq!(hit_uids(&db, acc, "-subject:meeting is:read"), [113]);
    // Exclusions alone still search nothing.
    assert_eq!(hit_uids(&db, acc, "-newsletter"), Vec::<u32>::new());
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

#[test]
fn answer_draft_json_reads_the_stored_message_and_settings() {
    let (db, acc, f) = setup();
    msg_store::upsert(&db, &msg_store::sample_new(acc, f, 5)).unwrap();
    settings::set(&db, settings::SIGNATURE_ENABLED, "1").unwrap();
    settings::set(&db, settings::SIGNATURE_TEXT, "Bob").unwrap();

    let d: serde_json::Value =
        serde_json::from_str(&crate::compose::answer_draft_json(&db, f, 5, "reply_all").unwrap())
            .unwrap();
    assert_eq!(d["to"], "alice@example.com");
    assert_eq!(d["cc"], "bob@example.com");
    assert_eq!(d["subject"], "Re: Hello");
    assert_eq!(d["signature_text"], "-- \nBob");
    let quote = d["quote_html"].as_str().unwrap();
    assert!(quote.contains("Alice &lt;alice@example.com&gt; wrote:"));
    assert!(quote.contains("&gt; Hello Bob, how are you?"));
    // The attribution carries a full date, never the list's short form.
    assert!(quote.contains(&full_local_date(Some("2026-09-07T10:00:00+00:00"))));

    assert!(crate::compose::answer_draft_json(&db, f, 5, "bogus").is_err());
}

#[test]
fn folders_say_whether_delete_destroys() {
    let (db, acc, _inbox) = setup();
    let row = |db: &Db, name: &str| -> serde_json::Value {
        let all: serde_json::Value = serde_json::from_str(&folders_json(db, acc).unwrap()).unwrap();
        all.as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == name)
            .cloned()
            .unwrap()
    };
    // No Trash yet: every delete destroys.
    assert_eq!(row(&db, "INBOX")["delete_is_permanent"], true);
    folders::upsert(&db, acc, "Trash", "/", FolderRole::Trash).unwrap();
    folders::upsert(&db, acc, "Junk", "/", FolderRole::Junk).unwrap();
    assert_eq!(row(&db, "INBOX")["delete_is_permanent"], false);
    assert_eq!(row(&db, "Trash")["delete_is_permanent"], true);
    assert_eq!(row(&db, "Junk")["delete_is_permanent"], true);
}

#[test]
fn show_older_follows_cache_and_server_counts() {
    assert_eq!(older_state(10, None), OlderState::Unchecked);
    assert_eq!(older_state(10, Some(25)), OlderState::Partial);
    assert_eq!(older_state(0, Some(25)), OlderState::Partial);
    assert_eq!(older_state(0, Some(0)), OlderState::Empty);
    assert_eq!(older_state(25, Some(25)), OlderState::Complete);
    assert!(OlderState::Unchecked.can_load() && OlderState::Partial.can_load());
    assert!(!OlderState::Empty.can_load() && !OlderState::Complete.can_load());
    assert_eq!(
        older_label(10, None, false),
        "Cached 10 (server not checked)"
    );
    assert_eq!(older_label(10, Some(25), false), "Cached 10 of 25");
    assert_eq!(older_label(25, Some(25), false), "All 25 messages loaded");
    assert_eq!(older_label(0, Some(0), true), "");
    assert_eq!(
        older_label(10, Some(25), true),
        "Cached 10 of 25 · filters cover loaded mail only"
    );

    let (db, acc, _inbox) = setup();
    let all: serde_json::Value = serde_json::from_str(&folders_json(&db, acc).unwrap()).unwrap();
    assert_eq!(all[0]["server_total"], -1);
    assert_eq!(all[0]["older"], "unchecked");
    assert_eq!(all[0]["can_load_older"], true);
}

#[test]
fn message_json_includes_calendar_event() {
    let (db, acc, f) = setup();
    let m = msg_store::sample_new(acc, f, 101);
    let msg_id = msg_store::upsert(&db, &m).unwrap();

    let ics_bytes = b"BEGIN:VCALENDAR\r\n\
BEGIN:VEVENT\r\n\
DTSTART:20261006T140000Z\r\n\
DTEND:20261006T150000Z\r\n\
SUMMARY:Quarterly Review\r\n\
LOCATION:Room 1\r\n\
ORGANIZER;CN=Host:mailto:host@example.org\r\n\
STATUS:CONFIRMED\r\n\
END:VEVENT\r\n\
END:VCALENDAR\r\n";

    let att = crate::models::NewAttachment {
        filename: Some("invite.ics".to_string()),
        mime_type: Some("text/calendar".to_string()),
        content_id: None,
        size: ics_bytes.len() as u64,
        data: Some(ics_bytes.to_vec()),
        is_inline: false,
    };
    let att_id = msg_store::add_attachment(&db, msg_id, &att).unwrap();

    let reader: serde_json::Value =
        serde_json::from_str(&message_json(&db, f, 101).unwrap()).unwrap();
    assert!(!reader["event"].is_null());
    assert_eq!(reader["event"]["summary"], "Quarterly Review");
    assert_eq!(reader["event"]["location"], "Room 1");
    assert_eq!(reader["event"]["organizer"], "Host <host@example.org>");
    assert_eq!(reader["event"]["attachment_id"], att_id);
    assert_eq!(reader["event"]["save_name"], "invite.ics");
    assert!(reader["event"].get("start_iso").is_none());
    assert_eq!(reader["event"]["is_cancelled"], false);
    assert_eq!(reader["attachments"][0]["in_card"], true);
    assert_eq!(reader["contacts"], serde_json::json!([]));
}

#[test]
fn message_json_includes_contact_cards() {
    let (db, acc, f) = setup();
    let msg_id = msg_store::upsert(&db, &msg_store::sample_new(acc, f, 102)).unwrap();
    let vcf = b"BEGIN:VCARD
VERSION:3.0
FN:Jane Doe
EMAIL;TYPE=work:jane@example.com
TEL;TYPE=cell:+1 555 0100
END:VCARD
";
    let file = |name: &str, mime: &str, data: Option<&[u8]>| crate::models::NewAttachment {
        filename: Some(name.to_string()),
        mime_type: Some(mime.to_string()),
        content_id: None,
        size: 100,
        data: data.map(<[u8]>::to_vec),
        is_inline: false,
    };
    let card_id =
        msg_store::add_attachment(&db, msg_id, &file("jane.vcf", "text/x-vcard", Some(vcf)))
            .unwrap();
    let pending_id =
        msg_store::add_attachment(&db, msg_id, &file("team.vcf", "text/vcard", None)).unwrap();
    // A .vcf that holds no card stays a plain attachment.
    let broken_id = msg_store::add_attachment(
        &db,
        msg_id,
        &file("broken.vcf", "text/vcard", Some(b"not a card")),
    )
    .unwrap();

    let reader: serde_json::Value =
        serde_json::from_str(&message_json(&db, f, 102).unwrap()).unwrap();
    let cards = reader["contacts"].as_array().unwrap();
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0]["name"], "Jane Doe");
    assert_eq!(cards[0]["loaded"], true);
    assert_eq!(cards[0]["attachment_id"], card_id);
    assert_eq!(cards[0]["save_name"], "jane.vcf");
    assert_eq!(cards[0]["emails"][0]["value"], "jane@example.com");
    assert_eq!(cards[0]["emails"][0]["label"], "Work");
    assert_eq!(cards[0]["phones"][0]["label"], "Mobile");
    assert_eq!(cards[0]["initials"], "JE");
    assert_eq!(cards[1]["name"], "team.vcf");
    assert_eq!(cards[1]["loaded"], false);
    assert_eq!(cards[1]["attachment_id"], pending_id);

    let in_card = |id: i64| {
        reader["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == id)
            .unwrap()["in_card"]
            .clone()
    };
    assert_eq!(in_card(card_id), true);
    assert_eq!(in_card(pending_id), true);
    assert_eq!(in_card(broken_id), false);
}

#[test]
fn sidebar_rows_fold_and_aggregate() {
    let (db, acc, _inbox) = setup();
    let projects = folders::upsert(&db, acc, "Projects", "/", FolderRole::Custom).unwrap();
    let client = folders::upsert(&db, acc, "Projects/Client", "/", FolderRole::Custom).unwrap();
    let deep = folders::upsert(&db, acc, "Projects/Client/Deep", "/", FolderRole::Custom).unwrap();
    // Two unread in the child, one unread in the grandchild, one read mail
    // of the parent's own.
    for uid in [11u32, 12] {
        msg_store::upsert(&db, &msg_store::sample_new(acc, client, uid)).unwrap();
    }
    msg_store::upsert(&db, &msg_store::sample_new(acc, deep, 13)).unwrap();
    let mut own = msg_store::sample_new(acc, projects, 14);
    own.is_read = true;
    msg_store::upsert(&db, &own).unwrap();

    let rows = |expanded: &[i64]| {
        serde_json::from_str::<serde_json::Value>(&sidebar_rows_json(&db, acc, expanded).unwrap())
            .unwrap()
    };
    let by_id = |v: &serde_json::Value, id: i64| {
        v.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()
            .clone()
    };
    let shown_ids = |v: &serde_json::Value| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_i64().unwrap())
            .collect::<Vec<_>>()
    };

    // Collapsed (default): descendants fold away transitively and the
    // parent carries their counts, so no unread pill disappears.
    let collapsed = rows(&[]);
    let p = by_id(&collapsed, projects);
    assert_eq!(p["collapsible"], true);
    assert_eq!(p["expanded"], false);
    assert_eq!(p["unread"], 3);
    assert_eq!(p["total"], 4);
    assert!(!shown_ids(&collapsed).contains(&client));
    assert!(!shown_ids(&collapsed).contains(&deep));

    // Expanded: every row carries only its own counts.
    let open = rows(&[projects]);
    let p = by_id(&open, projects);
    assert_eq!(p["expanded"], true);
    assert_eq!(p["unread"], 0);
    assert_eq!(p["total"], 1);
    let c = by_id(&open, client);
    assert_eq!(c["collapsible"], true);
    assert_eq!(c["expanded"], false);
    assert_eq!(c["unread"], 3);
    assert_eq!(c["total"], 3);
    assert!(!shown_ids(&open).contains(&deep));
}

#[test]
fn sidebar_rows_show_well_known_skip_hidden_and_split_delimiters() {
    let (db, acc, _inbox) = setup();
    let work = folders::upsert(&db, acc, "INBOX/Work", "/", FolderRole::Custom).unwrap();
    let trash = folders::upsert(&db, acc, "Trash", "/", FolderRole::Trash).unwrap();
    // Unread mail in Trash never pills, collapsed or not.
    msg_store::upsert(&db, &msg_store::sample_new(acc, trash, 21)).unwrap();
    // Dotted hierarchy resolves on its real delimiter.
    let lists = folders::upsert(&db, acc, "Lists", ".", FolderRole::Custom).unwrap();
    let rust = folders::upsert(&db, acc, "Lists.Rust", ".", FolderRole::Custom).unwrap();
    // No hierarchy at all: a root like any other.
    let flat = folders::upsert(&db, acc, "Flat", "", FolderRole::Custom).unwrap();
    // An unsubscribed folder leaves the tree; its custom child reads as a
    // root, the way the old flat list showed it.
    let hidden = folders::upsert(&db, acc, "Hidden", "/", FolderRole::Custom).unwrap();
    let orphan = folders::upsert(&db, acc, "Hidden/Child", "/", FolderRole::Custom).unwrap();
    folders::set_subscribed(&db, hidden, false).unwrap();

    let rows =
        serde_json::from_str::<serde_json::Value>(&sidebar_rows_json(&db, acc, &[]).unwrap())
            .unwrap();
    let by_id = |id: i64| {
        rows.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()
            .clone()
    };
    let shown: Vec<i64> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_i64().unwrap())
        .collect();

    // The inbox's direct custom child stays visible inside the collapsed
    // inbox and has nothing to fold, so no chevron and inbox-only counts.
    assert!(shown.contains(&work));
    assert_eq!(by_id(work)["collapsible"], false);
    assert_eq!(by_id(trash)["unread"], 0);
    // Dotted child folds into its parent on the "." delimiter.
    assert!(!shown.contains(&rust));
    assert_eq!(by_id(lists)["collapsible"], true);
    assert!(shown.contains(&flat));
    assert!(!shown.contains(&hidden));
    assert!(shown.contains(&orphan));
}

fn part(name: Option<&str>, mime: &str, data: Option<&[u8]>) -> crate::models::NewAttachment {
    crate::models::NewAttachment {
        filename: name.map(str::to_string),
        mime_type: Some(mime.to_string()),
        content_id: None,
        size: data.map_or(100, |d| d.len() as u64),
        data: data.map(<[u8]>::to_vec),
        is_inline: false,
    }
}

#[test]
fn bounce_shows_a_report_card_and_resends_the_sent_original() {
    let (db, acc, inbox) = setup();
    let sent = folders::upsert(&db, acc, "Sent", "/", FolderRole::Sent).unwrap();
    let mut original = msg_store::sample_new(acc, sent, 40);
    original.message_id_header = Some("orig@example.com".to_string());
    original.subject = Some("Project plan".to_string());
    original.to_addrs = vec![
        "bob@example.net".to_string(),
        "carol@example.net".to_string(),
    ];
    original.body_html = Some("<p>The plan.</p><p>-- <br>Alice</p>".to_string());
    let original_id = msg_store::upsert(&db, &original).unwrap();
    msg_store::add_attachment(
        &db,
        original_id,
        &part(Some("plan.pdf"), "application/pdf", Some(b"%PDF")),
    )
    .unwrap();

    let bounce_id = msg_store::upsert(&db, &msg_store::sample_new(acc, inbox, 41)).unwrap();
    let status = b"Reporting-MTA: dns; mx.example.org\r\n\r\n\
Final-Recipient: rfc822; bob@example.net\r\nAction: failed\r\nStatus: 5.1.1\r\n\
Diagnostic-Code: smtp; 550 5.1.1 User unknown\r\n\r\n\
Final-Recipient: rfc822; carol@example.net\r\nAction: delivered\r\nStatus: 2.0.0\r\n";
    let status_id = msg_store::add_attachment(
        &db,
        bounce_id,
        &part(None, "message/delivery-status", Some(status)),
    )
    .unwrap();
    let headers_id = msg_store::add_attachment(
        &db,
        bounce_id,
        &part(
            None,
            "text/rfc822-headers",
            Some(b"Message-ID: <orig@example.com>\r\nSubject: Project plan\r\n"),
        ),
    )
    .unwrap();

    let reader: serde_json::Value =
        serde_json::from_str(&message_json(&db, inbox, 41).unwrap()).unwrap();
    let report = &reader["report"];
    assert_eq!(report["outcome"], "failed");
    assert_eq!(report["title"], "Delivery failed");
    assert_eq!(report["loaded"], true);
    assert_eq!(report["recipients"][0]["address"], "bob@example.net");
    assert_eq!(
        report["recipients"][0]["reason"],
        "The address does not exist (5.1.1)"
    );
    assert_eq!(report["recipients"][0]["action_label"], "Failed");
    assert_eq!(report["recipients"][1]["action"], "delivered");
    assert_eq!(report["original_subject"], "Project plan");
    assert_eq!(report["original_folder_id"], sent);
    assert_eq!(report["original_uid"], 40);
    assert_eq!(report["can_resend"], true);
    assert!(report.get("covered").is_none());
    for a in reader["attachments"].as_array().unwrap() {
        assert!(a["id"] == status_id || a["id"] == headers_id);
        assert_eq!(a["in_card"], true);
    }

    // Resend goes to the failed recipient only, with the original's body
    // and files.
    let draft: serde_json::Value =
        serde_json::from_str(&crate::compose::answer_draft_json(&db, inbox, 41, "resend").unwrap())
            .unwrap();
    assert_eq!(draft["to"], "bob@example.net");
    assert_eq!(draft["subject"], "Project plan");
    assert!(draft["body_html"].as_str().unwrap().contains("The plan."));
    assert_eq!(draft["quote_html"], "");
    assert_eq!(crate::compose::resend_missing(&db, inbox, 41).unwrap(), 0);
    let dir = tempfile::tempdir().unwrap();
    let files = crate::compose::stage_resend_files(&db, inbox, 41, dir.path()).unwrap();
    assert_eq!(files.files.len(), 1);
    assert_eq!(files.files[0].name, "plan.pdf");

    // Not a bounce: no report, and no resend.
    assert!(crate::compose::answer_draft_json(&db, sent, 40, "resend").is_err());
    let plain: serde_json::Value =
        serde_json::from_str(&message_json(&db, sent, 40).unwrap()).unwrap();
    assert!(plain["report"].is_null());
}

#[test]
fn uncached_report_is_pending_and_unknown_original_cannot_resend() {
    let (db, acc, inbox) = setup();
    let id = msg_store::upsert(&db, &msg_store::sample_new(acc, inbox, 50)).unwrap();
    msg_store::add_attachment(&db, id, &part(None, "message/delivery-status", None)).unwrap();
    let reader: serde_json::Value =
        serde_json::from_str(&message_json(&db, inbox, 50).unwrap()).unwrap();
    assert_eq!(reader["report"]["loaded"], false);
    assert_eq!(reader["report"]["can_resend"], false);

    let id = msg_store::upsert(&db, &msg_store::sample_new(acc, inbox, 51)).unwrap();
    msg_store::add_attachment(
        &db,
        id,
        &part(
            None,
            "message/delivery-status",
            Some(b"Final-Recipient: rfc822; x@example.net\r\nAction: failed\r\n"),
        ),
    )
    .unwrap();
    let reader: serde_json::Value =
        serde_json::from_str(&message_json(&db, inbox, 51).unwrap()).unwrap();
    assert_eq!(reader["report"]["outcome"], "failed");
    assert_eq!(reader["report"]["can_resend"], false);
    assert!(reader["report"]["original_uid"].is_null());
}

#[test]
fn attached_mails_become_cards() {
    let (db, acc, f) = setup();
    let id = msg_store::upsert(&db, &msg_store::sample_new(acc, f, 60)).unwrap();
    let eml = b"From: Jane <jane@example.com>\r\nSubject: Inner\r\nDate: Tue, 6 Oct 2026 14:00:00 +0000\r\n\r\nInner body\r\n";
    let loaded =
        msg_store::add_attachment(&db, id, &part(None, "message/rfc822", Some(eml))).unwrap();
    let pending =
        msg_store::add_attachment(&db, id, &part(Some("old.eml"), "message/rfc822", None)).unwrap();
    let junk = msg_store::add_attachment(
        &db,
        id,
        &part(Some("junk.eml"), "message/rfc822", Some(b"\x00\x01")),
    )
    .unwrap();

    let reader: serde_json::Value =
        serde_json::from_str(&message_json(&db, f, 60).unwrap()).unwrap();
    let cards = reader["attached_messages"].as_array().unwrap();
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0]["subject"], "Inner");
    assert_eq!(cards[0]["from"], "Jane <jane@example.com>");
    assert_eq!(cards[0]["body_text"], "Inner body");
    assert_eq!(cards[0]["attachment_id"], loaded);
    assert_eq!(cards[1]["loaded"], false);
    assert_eq!(cards[1]["subject"], "old.eml");
    let in_card = |want: i64| {
        reader["attachments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == want)
            .unwrap()["in_card"]
            .clone()
    };
    assert_eq!(in_card(loaded), true);
    assert_eq!(in_card(pending), true);
    assert_eq!(in_card(junk), false);
}
