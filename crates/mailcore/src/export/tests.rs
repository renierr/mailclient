use super::*;
use crate::db::Db;
use crate::models::{Attachment, FolderRole, NewAccount, NewMessage};
use crate::store::{accounts, folders, messages};
use mail_parser::MimeHeaders;

fn make_test_db() -> (Db, i64, i64) {
    let db = Db::open_in_memory().unwrap();
    let acc_id = accounts::create(
        &db,
        &NewAccount {
            name: "Work".into(),
            email_address: "me@example.com".into(),
            from_name: "Me".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            imap_security: "tls".into(),
            imap_username: "me".into(),
            smtp_host: "smtp.example.com".into(),
            smtp_port: 465,
            smtp_security: "tls".into(),
            smtp_username: "me".into(),
            auth_vault_key: "k".into(),
            check_interval_secs: 300,
        },
    )
    .unwrap();
    let folder_id = folders::upsert(&db, acc_id, "INBOX", "/", FolderRole::Inbox).unwrap();
    (db, acc_id, folder_id)
}

#[test]
fn filename_sanitization() {
    assert_eq!(
        safe_eml_filename(Some("Project Update / Q3: Review"), 42),
        "Project Update _ Q3_ Review.eml"
    );
    assert_eq!(safe_eml_filename(None, 42), "message-42.eml");
    assert_eq!(
        safe_eml_filename(Some("(no subject)"), 42),
        "message-42.eml"
    );
    assert_eq!(safe_eml_filename(Some("   "), 42), "message-42.eml");
    assert_eq!(safe_eml_filename(Some("..."), 42), "message-42.eml");
}

#[test]
fn assemble_plain_message_parses_with_mail_parser() {
    let msg = Message {
        id: 1,
        account_id: 1,
        folder_id: 1,
        uid: 101,
        message_id_header: Some("msg-101@example.com".into()),
        thread_id: None,
        subject: Some("Plain test".into()),
        from_addr: Some("sender@example.com".into()),
        from_name: Some("Sender Name".into()),
        to_addrs: vec!["recipient@example.com".into()],
        cc_addrs: vec![],
        bcc_addrs: vec![],
        reply_to: None,
        date: Some("2026-10-03T10:00:00Z".into()),
        snippet: Some("Hello plain".into()),
        body_text: Some("Hello plain body\nSecond line".into()),
        body_html: None,
        raw_headers: None,
        is_read: true,
        is_starred: false,
        is_draft: false,
        has_attachments: false,
        keywords: vec![],
        size: 100,
        downloaded_full: true,
    };

    let bytes = assemble_message_eml(&msg, &[]).unwrap();
    let parsed = mail_parser::MessageParser::default().parse(&bytes).unwrap();

    assert_eq!(parsed.subject().unwrap(), "Plain test");
    assert_eq!(
        parsed.from().unwrap().first().unwrap().address().unwrap(),
        "sender@example.com"
    );
    assert!(parsed.body_text(0).unwrap().contains("Hello plain body"));
    assert!(parsed.body_text(0).unwrap().contains("Second line"));
}

#[test]
fn assemble_multipart_with_attachments_parses_cleanly() {
    let msg = Message {
        id: 2,
        account_id: 1,
        folder_id: 1,
        uid: 102,
        message_id_header: Some("msg-102@example.com".into()),
        thread_id: None,
        subject: Some("Invoice & Receipt".into()),
        from_addr: Some("billing@example.com".into()),
        from_name: None,
        to_addrs: vec!["client@example.com".into()],
        cc_addrs: vec!["accounting@example.com".into()],
        bcc_addrs: vec![],
        reply_to: None,
        date: Some("2026-10-03T12:00:00Z".into()),
        snippet: Some("Invoice details".into()),
        body_text: Some("Please find attached.".into()),
        body_html: Some("<p>Please find <b>attached</b>.</p>".into()),
        raw_headers: Some("X-Custom: verified\r\nSubject: Invoice & Receipt\r\nFrom: billing@example.com\r\nContent-Type: old/type\r\n".into()),
        is_read: true,
        is_starred: true,
        is_draft: false,
        has_attachments: true,
        keywords: vec![],
        size: 250,
        downloaded_full: true,
    };

    let attachment = Attachment {
        id: 1,
        message_id: 2,
        filename: Some("invoice.pdf".into()),
        mime_type: Some("application/pdf".into()),
        size: 4,
        content_id: None,
        data: Some(b"%PDF".to_vec()),
        is_inline: false,
        storage_path: None,
    };

    let bytes = assemble_message_eml(&msg, &[attachment]).unwrap();
    let parsed = mail_parser::MessageParser::default().parse(&bytes).unwrap();

    assert_eq!(parsed.subject().unwrap(), "Invoice & Receipt");
    assert_eq!(
        parsed.header("X-Custom").and_then(|h| h.as_text()),
        Some("verified")
    );
    assert!(parsed
        .body_text(0)
        .unwrap()
        .contains("Please find attached."));
    assert!(parsed.body_html(0).unwrap().contains("<b>attached</b>"));

    let attachments: Vec<_> = parsed.attachments().collect();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].attachment_name(), Some("invoice.pdf"));
    assert_eq!(attachments[0].contents(), b"%PDF");
}

#[test]
fn export_eml_to_creates_file_in_dir() {
    let (db, acc_id, folder_id) = make_test_db();
    let msg = NewMessage {
        account_id: acc_id,
        folder_id,
        uid: 103,
        message_id_header: None,
        thread_id: None,
        subject: Some("Report".into()),
        from_addr: Some("boss@example.com".into()),
        from_name: None,
        to_addrs: vec!["team@example.com".into()],
        cc_addrs: vec![],
        bcc_addrs: vec![],
        reply_to: None,
        date: Some("2026-10-03T14:00:00Z".into()),
        snippet: Some("Quarterly".into()),
        body_text: Some("Quarterly results".into()),
        body_html: None,
        raw_headers: None,
        is_read: true,
        is_starred: false,
        is_draft: false,
        has_attachments: false,
        keywords: vec![],
        size: 50,
        downloaded_full: true,
    };
    messages::upsert(&db, &msg).unwrap();

    let temp = tempfile::tempdir().unwrap();
    let out_path = export_eml_to(&db, folder_id, 103, temp.path().to_str().unwrap()).unwrap();
    assert!(out_path.exists());
    assert_eq!(out_path.file_name().unwrap(), "Report.eml");

    let content = std::fs::read(&out_path).unwrap();
    assert!(content.starts_with(b"Date:"));
    assert!(String::from_utf8_lossy(&content).contains("Quarterly results"));
}

fn message(raw_headers: Option<&str>) -> Message {
    Message {
        id: 9,
        account_id: 1,
        folder_id: 1,
        uid: 109,
        message_id_header: Some("msg-109@example.com".into()),
        thread_id: None,
        subject: Some("Grüße \"quoted\" – Bericht".into()),
        from_addr: Some("sender@example.com".into()),
        from_name: Some("Doe, \"J\" Ä".into()),
        to_addrs: vec!["recipient@example.com".into()],
        cc_addrs: vec![],
        bcc_addrs: vec![],
        reply_to: None,
        date: Some("2026-10-03T10:00:00Z".into()),
        snippet: None,
        body_text: Some("plain".into()),
        body_html: Some("<p>html <img src=\"cid:logo@example.com\"></p>".into()),
        raw_headers: raw_headers.map(str::to_string),
        is_read: true,
        is_starred: false,
        is_draft: false,
        has_attachments: true,
        keywords: vec![],
        size: 10,
        downloaded_full: true,
    }
}

fn file(id: i64, name: &str, data: Option<&[u8]>, cid: Option<&str>) -> Attachment {
    Attachment {
        id,
        message_id: 9,
        filename: Some(name.into()),
        mime_type: Some(
            if cid.is_some() {
                "image/png"
            } else {
                "application/pdf"
            }
            .into(),
        ),
        size: 4,
        content_id: cid.map(str::to_string),
        data: data.map(<[u8]>::to_vec),
        is_inline: cid.is_some(),
        storage_path: None,
    }
}

#[test]
fn untrusted_filenames_and_headers_are_encoded_not_injected() {
    let files = [
        file(1, "a\"b\r\nX-Injected: 1.pdf", Some(b"%PDF"), None),
        file(2, "Grüße – Bericht.pdf", Some(b"%PDF"), None),
    ];
    let bytes = assemble_message_eml(&message(None), &files).unwrap();
    let parsed = mail_parser::MessageParser::default().parse(&bytes).unwrap();
    assert!(parsed.header("X-Injected").is_none());
    assert!(!String::from_utf8_lossy(&bytes).contains("\nX-Injected"));
    let names: Vec<_> = parsed
        .attachments()
        .filter_map(|a| a.attachment_name())
        .collect();
    assert!(names.contains(&"Grüße – Bericht.pdf"), "{names:?}");
    assert_eq!(parsed.subject(), Some("Grüße \"quoted\" – Bericht"));
    let from = parsed.from().unwrap().first().unwrap();
    assert_eq!(from.name(), Some("Doe, \"J\" Ä"));
    assert_eq!(from.address(), Some("sender@example.com"));
    assert!(bytes.is_ascii(), "every header and body is 7-bit encoded");
}

#[test]
fn missing_attachment_bytes_refuse_instead_of_placeholders() {
    let files = [file(1, "report.pdf", None, None)];
    let err = assemble_message_eml(&message(None), &files).unwrap_err();
    assert!(err.to_string().contains("not downloaded"), "{err}");
}

#[test]
fn inline_images_travel_in_multipart_related() {
    let files = [file(
        1,
        "logo.png",
        Some(b"\x89PNG"),
        Some("<logo@example.com>"),
    )];
    let bytes = assemble_message_eml(&message(None), &files).unwrap();
    let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
    assert!(text.contains("multipart/related"));
    assert!(text.contains("content-id: <logo@example.com>"));
}

#[test]
fn raw_headers_keep_continuations_and_drop_old_mime_headers() {
    let raw = "Received: from a.example.com\r\n\tby b.example.com\r\nContent-Type: text/plain;\r\n charset=x-old\r\nX-Kept: 1\r\n";
    let out = filter_raw_headers(raw);
    assert_eq!(
        out,
        "Received: from a.example.com\r\n\tby b.example.com\r\nX-Kept: 1\r\n"
    );
}

#[test]
fn long_body_lines_stay_within_the_rfc_limit() {
    let mut m = message(None);
    m.body_html = Some(format!("<p>{}</p>", "x".repeat(5000)));
    let bytes = assemble_message_eml(&m, &[]).unwrap();
    let longest = bytes.split(|b| *b == b'\n').map(<[u8]>::len).max().unwrap();
    assert!(longest <= 998, "{longest}");
}

#[test]
fn eml_names_follow_the_attachment_rules() {
    assert_eq!(safe_eml_filename(Some("CON"), 1), "_CON.eml");
    assert_eq!(safe_eml_filename(Some("a/b\\c"), 1), "a_b_c.eml");
    assert!(safe_eml_filename(Some(&"ü".repeat(200)), 1).len() <= 150);
}
