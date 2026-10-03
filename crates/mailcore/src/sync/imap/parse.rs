//! RFC822 → local models: flag mapping, body/snippet extraction, attachment
//! metadata, and contact collection.

use imap_types::flag::Flag;

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::html::{is_inline_image_mime, MAX_INLINE_BYTES_PER_MESSAGE, MAX_INLINE_IMAGE_BYTES};
use crate::models::{NewAttachment, NewMessage};
use crate::store::{account_settings, contacts, messages, settings};

use super::types::{MAX_ATTACHMENTS_PER_MESSAGE, MAX_ATTACHMENT_BYTES};

pub(crate) fn flag_state(flags: &[Flag<'static>]) -> (bool, bool, bool) {
    let mut read = false;
    let mut starred = false;
    let mut draft = false;
    for f in flags {
        match f {
            Flag::Seen => read = true,
            Flag::Flagged => starred = true,
            Flag::Draft => draft = true,
            _ => {}
        }
    }
    (read, starred, draft)
}

pub(crate) fn parse_to_new(
    account_id: i64,
    folder_id: i64,
    uid: u32,
    flags: &[Flag<'static>],
    raw: &[u8],
    with_bytes: bool,
) -> Result<(NewMessage, Vec<NewAttachment>)> {
    let parsed = mail_parser::MessageParser::default()
        .parse(raw)
        .ok_or_else(|| StoreError::InvalidInput(format!("cannot parse message uid {uid}")))?;
    let (is_read, is_starred, is_draft) = flag_state(flags);
    let is_read = is_read || is_draft;

    let body_text = parsed.body_text(0).map(|c| c.into_owned());
    let snippet = body_text.as_deref().map(|t| {
        let flat: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
        flat.chars().take(200).collect()
    });
    let date = parsed
        .date()
        .map(|d| d.to_timestamp())
        .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));

    let thread_id = parsed
        .header("References")
        .and_then(|h| h.as_text())
        .and_then(|t| t.split_whitespace().last().map(str::to_string))
        .or_else(|| {
            parsed
                .header("In-Reply-To")
                .and_then(|h| h.as_text())
                .map(str::to_string)
        });

    let body_html = real_html_body(&parsed);
    // `extract_attachments` marks `cid:`-shown parts inline (even under
    // `Content-Disposition: attachment`, as newsletters send logos), so the
    // same image neither lists as a file nor raises the flag.
    let files = extract_attachments(&parsed, with_bytes, body_html.as_deref());
    // `attachment_count` is the untruncated part total, so parts skipped
    // above (over the count/size caps) still raise the flag — while a mail
    // of only body images does not.
    let has_attachments =
        files.iter().any(|f| !f.is_inline) || parsed.attachment_count() > files.len();

    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len());
    let raw_headers = String::from_utf8_lossy(&raw[..header_end]).to_string();

    Ok((
        NewMessage {
            account_id,
            folder_id,
            uid,
            message_id_header: parsed.message_id().map(str::to_string),
            thread_id,
            subject: parsed.subject().map(str::to_string),
            from_addr: parsed
                .from()
                .and_then(|a| a.first())
                .and_then(|a| a.address.as_ref().map(|s| s.to_string()))
                .or_else(|| {
                    parsed
                        .header("From")
                        .and_then(|h| h.as_text())
                        .map(str::to_string)
                }),
            from_name: parsed
                .from()
                .and_then(|a| a.first())
                .and_then(|a| a.name.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            to_addrs: addr_list(parsed.to()),
            cc_addrs: addr_list(parsed.cc()),
            bcc_addrs: addr_list(parsed.bcc()),
            reply_to: parsed
                .reply_to()
                .and_then(|a| a.first())
                .and_then(|a| a.address.as_ref().map(|s| s.to_string())),
            date,
            snippet,
            body_text,
            body_html,
            raw_headers: (!raw_headers.is_empty()).then_some(raw_headers),
            is_read,
            is_starred,
            is_draft,
            has_attachments,
            keywords: Vec::new(),
            size: raw.len() as u64,
            downloaded_full: true,
        },
        files,
    ))
}

/// The HTML part, if the mail has one. `body_html()` alone is not enough:
/// for a plain-text mail mail-parser converts the text part into
/// `<html><body>…<br/>` on the fly, and storing that made every plain mail
/// render as HTML.
pub(crate) fn real_html_body(parsed: &mail_parser::Message<'_>) -> Option<String> {
    let part = parsed.html_part(0)?;
    matches!(part.body, mail_parser::PartType::Html(_))
        .then(|| parsed.body_html(0).map(|c| c.into_owned()))
        .flatten()
}

pub(crate) fn extract_attachments(
    parsed: &mail_parser::Message<'_>,
    with_bytes: bool,
    body_html: Option<&str>,
) -> Vec<NewAttachment> {
    use mail_parser::{MimeHeaders, PartType};
    let mut out = Vec::new();
    for part in parsed.attachments().take(MAX_ATTACHMENTS_PER_MESSAGE) {
        let len = match &part.body {
            PartType::Binary(b) | PartType::InlineBinary(b) => b.len(),
            PartType::Text(t) | PartType::Html(t) => t.len(),
            PartType::Message(nested) => nested.raw_message.len(),
            PartType::Multipart(_) => 0,
        };
        if len > MAX_ATTACHMENT_BYTES {
            continue;
        }
        if len == 0 {
            continue;
        }
        let mime_type = part.content_type().map(|ct| match &ct.c_subtype {
            Some(sub) => format!(
                "{}/{}",
                ct.c_type.to_ascii_lowercase(),
                sub.to_ascii_lowercase()
            ),
            None => ct.c_type.to_ascii_lowercase(),
        });
        let content_id = part.content_id().map(str::to_string);
        let data: Option<Vec<u8>> = if with_bytes || is_inline_image(&content_id, &mime_type, len) {
            match &part.body {
                PartType::Binary(b) | PartType::InlineBinary(b) => Some(b.to_vec()),
                PartType::Text(t) | PartType::Html(t) => Some(t.as_bytes().to_vec()),
                PartType::Message(nested) => Some(nested.raw_message.to_vec()),
                PartType::Multipart(_) => None,
            }
        } else {
            None
        };
        if with_bytes && data.as_ref().is_none_or(|b| b.is_empty()) {
            continue;
        }
        // Bytes in hand beat the header: a confident `image/jpeg` on PNG
        // bytes (or `octet-stream` on anything) is corrected, so the file
        // written later opens with the right application.
        let mime_type = match data.as_deref() {
            Some(bytes) => crate::mime::corrected_mime(mime_type.as_deref(), bytes).or(mime_type),
            None => mime_type,
        };
        let mut is_inline = matches!(part.body, PartType::InlineBinary(_));
        // Same rule as `parse_to_new`, for callers holding no message row
        // (the on-demand download): a `cid:`-shown part must not flip back
        // to a listed file on re-fetch.
        if !is_inline && crate::html::is_body_referenced(content_id.as_deref(), body_html) {
            is_inline = true;
        }
        out.push(NewAttachment {
            filename: part.attachment_name().map(str::to_string),
            mime_type,
            content_id,
            size: len as u64,
            data,
            is_inline,
        });
    }
    out
}

pub(crate) fn store_attachments(db: &Db, message_id: i64, files: Vec<NewAttachment>) -> Result<()> {
    messages::replace_attachments(db, message_id, &files)
}

/// A part the HTML body can show by `cid:`. Its bytes are kept at sync time
/// even though other attachments store metadata only: the full message is
/// already in memory, and opening a mail must never need the network.
fn is_inline_image(content_id: &Option<String>, mime: &Option<String>, len: usize) -> bool {
    content_id.as_deref().is_some_and(|c| !c.trim().is_empty())
        && mime.as_deref().is_some_and(is_inline_image_mime)
        && len <= MAX_INLINE_IMAGE_BYTES
}

/// Store attachment rows for a freshly synced message: names and sizes,
/// plus the bytes of inline images up to the per-message budget.
pub(crate) fn store_attachment_meta(db: &Db, message_id: i64, files: Vec<NewAttachment>) {
    if files.is_empty() {
        return;
    }
    match messages::list_attachments(db, message_id) {
        Ok(existing) if !existing.is_empty() => return,
        Err(e) => {
            log::warn!("imap: cannot list attachments for {message_id}: {e}");
            return;
        }
        _ => {}
    }
    let mut budget = MAX_INLINE_BYTES_PER_MESSAGE;
    for f in &files {
        let keep = f
            .data
            .as_ref()
            .filter(|d| is_inline_image(&f.content_id, &f.mime_type, d.len()) && d.len() <= budget);
        if let Some(d) = keep {
            budget -= d.len();
        }
        let meta = NewAttachment {
            data: keep.cloned(),
            ..f.clone()
        };
        if let Err(e) = messages::add_attachment(db, message_id, &meta) {
            log::warn!("imap: cannot store attachment {:?}: {e}", f.filename);
        }
    }
}

fn addr_list(a: Option<&mail_parser::Address>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(a) = a {
        for addr in a.iter() {
            if let Some(email) = addr.address.as_ref() {
                out.push(email.to_string());
            }
        }
    }
    out
}

pub(crate) fn collect_contacts_from_headers(db: &Db, account_id: i64, raw_headers: Option<&str>) {
    if !account_settings::get_bool(db, account_id, settings::COLLECT_SENT_CONTACTS) {
        return;
    }
    let Some(headers) = raw_headers else {
        return;
    };
    if headers.trim().is_empty() {
        return;
    }
    if let Some(parsed) = mail_parser::MessageParser::default().parse(headers.as_bytes()) {
        let collect = |addr_list: Option<&mail_parser::Address>| {
            if let Some(addrs) = addr_list {
                for a in addrs.iter() {
                    if let Some(email) = a.address.as_deref() {
                        let name = a.name.as_deref();
                        if let Err(e) = contacts::seen(db, email, name) {
                            log::warn!("contacts: could not collect contact {email}: {e}");
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

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::Db;
    use crate::models::{FolderRole, NewAttachment};
    use crate::store::{accounts, folders, messages};

    #[test]
    fn attachment_download_preserves_metadata_ids() {
        let db = Db::open_in_memory().unwrap();
        let account_id = accounts::create(
            &db,
            &crate::models::NewAccount {
                name: "Test".to_string(),
                email_address: "alice@example.com".to_string(),
                from_name: String::new(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "alice@example.com".to_string(),
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "alice@example.com".to_string(),
                auth_vault_key: "test".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let folder_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        let message_id =
            messages::upsert(&db, &messages::sample_new(account_id, folder_id, 1)).unwrap();
        let files: Vec<_> = [b"first".to_vec(), b"other".to_vec()]
            .into_iter()
            .map(|data| NewAttachment {
                filename: Some("notes.txt".to_string()),
                mime_type: Some("text/plain".to_string()),
                content_id: None,
                size: data.len() as u64,
                data: Some(data),
                is_inline: false,
            })
            .collect();
        store_attachment_meta(&db, message_id, files.clone());
        let metadata = messages::list_attachments(&db, message_id).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for _ in 0..2 {
            store_attachments(&db, message_id, files.clone()).unwrap();
            assert_eq!(
                messages::list_attachments(&db, message_id).unwrap().len(),
                2
            );
            for (original, expected) in metadata.iter().zip(&files) {
                let downloaded = messages::get_attachment(&db, original.id).unwrap();
                assert_eq!(downloaded.data, expected.data);
                let path = dir.path().join(original.id.to_string());
                messages::save_attachment_to_path(&db, original.id, &path).unwrap();
                assert_eq!(
                    std::fs::read(path).unwrap(),
                    expected.data.as_ref().unwrap().as_slice()
                );
            }
        }
    }

    #[test]
    fn sync_keeps_inline_image_bytes_and_the_reader_embeds_them() {
        let db = Db::open_in_memory().unwrap();
        let account_id = accounts::create(
            &db,
            &crate::models::NewAccount {
                name: "Test".to_string(),
                email_address: "alice@example.com".to_string(),
                from_name: String::new(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "alice@example.com".to_string(),
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "alice@example.com".to_string(),
                auth_vault_key: "test".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let folder_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        let raw = b"From: bob@example.org\r\nTo: alice@example.com\r\nSubject: logo\r\n\
Content-Type: multipart/mixed; boundary=\"M\"\r\n\r\n\
--M\r\nContent-Type: multipart/related; boundary=\"R\"\r\n\r\n\
--R\r\nContent-Type: text/html\r\n\r\n<p>hi<img src=\"cid:logo@example.org\" alt=\"Logo\"></p>\r\n\
--R\r\nContent-Type: image/png\r\nContent-ID: <logo@example.org>\r\n\
Content-Disposition: inline\r\nContent-Transfer-Encoding: base64\r\n\r\nZm9v\r\n\
--R--\r\n\
--M\r\nContent-Type: application/pdf; name=\"a.pdf\"\r\n\
Content-Disposition: attachment; filename=\"a.pdf\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nYmFy\r\n\
--M--\r\n";
        let (msg, files) = parse_to_new(account_id, folder_id, 5, &[], raw, false).unwrap();
        let id = messages::upsert(&db, &msg).unwrap();
        store_attachment_meta(&db, id, files);

        let stored = messages::list_attachments(&db, id).unwrap();
        let pdf = stored.iter().find(|a| a.content_id.is_none()).unwrap();
        assert!(!messages::attachment_has_data(&db, pdf.id).unwrap());
        assert_eq!(messages::inline_images(&db, id).unwrap().len(), 1);

        let json: serde_json::Value =
            serde_json::from_str(&crate::feed::message_json(&db, folder_id, 5).unwrap()).unwrap();
        let body = json["body_html"].as_str().unwrap();
        assert!(
            body.contains("src=\"data:image/png;base64,Zm9v\""),
            "{body}"
        );
        assert!(!body.contains("cid:"));
        assert_eq!(json["missing_inline_images"], 0);
    }

    #[test]
    fn plain_text_mail_has_no_html_body() {
        let raw = b"From: a@example.com
Subject: hi
Content-Type: text/plain

line one
line two
";
        let (msg, _) = parse_to_new(1, 1, 7, &[], raw, false).unwrap();
        assert_eq!(msg.body_html, None);
        assert!(msg.body_text.unwrap().contains("line two"));

        let raw = b"From: a@example.com
Content-Type: multipart/alternative; boundary=\"A\"

--A
Content-Type: text/plain

plain
--A
Content-Type: text/html

<p>rich</p>
--A--
";
        let (msg, _) = parse_to_new(1, 1, 8, &[], raw, false).unwrap();
        assert!(msg.body_html.unwrap().contains("<p>rich</p>"));
    }

    #[test]
    fn test_flag_state_mapping() {
        let (read, starred, draft) = flag_state(&[]);
        assert!(!read);
        assert!(!starred);
        assert!(!draft);

        let (read, starred, draft) = flag_state(&[Flag::Seen]);
        assert!(read);
        assert!(!starred);
        assert!(!draft);

        let (read, starred, draft) = flag_state(&[Flag::Seen, Flag::Flagged]);
        assert!(read);
        assert!(starred);
        assert!(!draft);

        let (read, starred, draft) = flag_state(&[Flag::Draft]);
        assert!(!read);
        assert!(!starred);
        assert!(draft);
    }

    #[test]
    fn snippet_collapses_multiline_bodies_to_one_line() {
        let raw = b"From: a@x.y\r\nSubject: hi\r\nContent-Type: text/plain\r\n\r\nline one\nline two\r\n\tline three";
        let (msg, _) = parse_to_new(1, 1, 7, &[], raw, false).unwrap();
        let snippet = msg.snippet.expect("text body has a snippet");
        assert!(!snippet.contains('\n'), "snippet must stay single-line");
        assert_eq!(snippet, "line one line two line three");
    }

    #[test]
    fn snippet_truncates_long_bodies_at_200_chars() {
        let body = "w ".repeat(500);
        let raw = format!("From: a@x.y\r\nContent-Type: text/plain\r\n\r\n{body}");
        let (msg, _) = parse_to_new(1, 1, 7, &[], raw.as_bytes(), false).unwrap();
        assert_eq!(msg.snippet.map(|s| s.chars().count()), Some(200));
    }

    #[test]
    fn draft_flag_implies_read_and_headers_only_has_no_snippet() {
        let raw = b"From: a@x.y\r\nTo: b@x.y\r\nSubject: draft\r\n\r\n";
        let (msg, _) = parse_to_new(1, 1, 7, &[Flag::Draft], raw, false).unwrap();
        assert!(msg.is_draft);
        assert!(msg.is_read, "drafts count as read");
        assert!(msg.snippet.as_deref().unwrap_or_default().is_empty());
        assert_eq!(msg.from_addr.as_deref(), Some("a@x.y"));
    }

    #[test]
    fn missing_from_header_yields_none_not_panic() {
        let raw = b"Subject: no sender\r\nContent-Type: text/plain\r\n\r\nbody";
        let (msg, _) = parse_to_new(1, 1, 7, &[], raw, false).unwrap();
        assert!(msg.from_addr.is_none());
        assert_eq!(msg.subject.as_deref(), Some("no sender"));
        assert!(!msg.is_read);
    }

    #[test]
    fn from_display_name_is_stored_decoded_next_to_the_address() {
        let raw = b"From: =?UTF-8?Q?J=C3=BCrgen_M=C3=BCller?= <juergen@example.com>\r\nSubject: hi\r\n\r\nbody";
        let (msg, _) = parse_to_new(1, 1, 7, &[], raw, false).unwrap();
        assert_eq!(msg.from_addr.as_deref(), Some("juergen@example.com"));
        assert_eq!(msg.from_name.as_deref(), Some("Jürgen Müller"));
    }

    #[test]
    fn address_only_from_has_no_display_name() {
        let raw = b"From: a@x.y\r\nSubject: hi\r\n\r\nbody";
        let (msg, _) = parse_to_new(1, 1, 7, &[], raw, false).unwrap();
        assert_eq!(msg.from_addr.as_deref(), Some("a@x.y"));
        assert!(msg.from_name.is_none());
    }

    #[test]
    fn single_attachment_sets_flag_with_and_without_bytes() {
        let raw = b"From: a@x.y\r\nTo: b@x.y\r\nSubject: files\r\nContent-Type: multipart/mixed; boundary=\"B\"\r\n\r\n--B\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n--B\r\nContent-Type: text/plain; name=\"n.txt\"\r\nContent-Disposition: attachment; filename=\"n.txt\"\r\nContent-Transfer-Encoding: base64\r\n\r\naGk=\r\n--B--\r\n";
        let (meta_msg, meta_files) = parse_to_new(1, 1, 7, &[], raw, false).unwrap();
        assert!(meta_msg.has_attachments);
        assert_eq!(meta_files.len(), 1);
        assert!(
            meta_files[0].data.is_none(),
            "metadata pass stores no bytes"
        );
        let (_, full_files) = parse_to_new(1, 1, 7, &[], raw, true).unwrap();
        assert_eq!(full_files.len(), 1);
        assert_eq!(full_files[0].data.as_deref(), Some(b"hi".as_slice()));
    }

    #[test]
    fn download_time_mime_fix_keeps_stable_ids() {
        let db = Db::open_in_memory().unwrap();
        let account_id = accounts::create(
            &db,
            &crate::models::NewAccount {
                name: "Test".to_string(),
                email_address: "alice@example.com".to_string(),
                from_name: String::new(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "alice@example.com".to_string(),
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "alice@example.com".to_string(),
                auth_vault_key: "test".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let folder_id = folders::upsert(&db, account_id, "INBOX", "/", FolderRole::Inbox).unwrap();
        let message_id =
            messages::upsert(&db, &messages::sample_new(account_id, folder_id, 1)).unwrap();
        // Sync-time metadata: header said octet-stream, no bytes to check.
        store_attachment_meta(
            &db,
            message_id,
            vec![NewAttachment {
                filename: Some("scan".to_string()),
                mime_type: Some("application/octet-stream".to_string()),
                content_id: None,
                size: 8,
                data: None,
                is_inline: false,
            }],
        );
        let before = messages::list_attachments(&db, message_id).unwrap();
        assert_eq!(before.len(), 1);
        // Download: magic says PDF. The row keeps its ID (the reader may
        // hold it), only the type is corrected in place.
        store_attachments(
            &db,
            message_id,
            vec![NewAttachment {
                filename: Some("scan".to_string()),
                mime_type: Some("application/pdf".to_string()),
                content_id: None,
                size: 8,
                data: Some(b"%PDF-1.7".to_vec()),
                is_inline: false,
            }],
        )
        .unwrap();
        let after = messages::list_attachments(&db, message_id).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, before[0].id);
        assert_eq!(after[0].mime_type.as_deref(), Some("application/pdf"));
    }

    #[test]
    fn attachment_disposition_shown_by_cid_is_a_body_part() {
        // The newsletter shape: logos declared `attachment` (filename
        // `inline`) but shown in the body via `cid:`. They must neither
        // list as files nor raise the list icon.
        let raw = b"From: info-noreply@example.com\r\nTo: a@x.y\r\nSubject: doc\r\n\
Content-Type: multipart/related; boundary=\"R\"\r\n\r\n\
--R\r\nContent-Type: text/html\r\n\r\n<p><img src=\"cid:yellowLogo\"></p>\r\n\
--R\r\nContent-Type: image/png\r\nContent-ID: yellowLogo\r\n\
Content-Disposition: attachment; filename=\"inline\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nZm9v\r\n\
--R--\r\n";
        let (msg, files) = parse_to_new(1, 1, 7, &[], raw, false).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].is_inline);
        assert!(!msg.has_attachments);
    }

    #[test]
    fn unreferenced_attachment_stays_a_file() {
        let raw = b"From: a@x.y\r\nTo: b@x.y\r\nSubject: files\r\n\
Content-Type: multipart/related; boundary=\"R\"\r\n\r\n\
--R\r\nContent-Type: text/html\r\n\r\n<p>no images here</p>\r\n\
--R\r\nContent-Type: image/png\r\nContent-ID: <logo@example.com>\r\n\
Content-Disposition: attachment; filename=\"inline\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\nZm9v\r\n\
--R--\r\n";
        let (msg, files) = parse_to_new(1, 1, 7, &[], raw, false).unwrap();
        assert_eq!(files.len(), 1);
        assert!(!files[0].is_inline);
        assert!(msg.has_attachments);
    }

    #[test]
    fn magic_bytes_correct_a_wrong_mime_header() {
        // Declared JPEG, actually PNG (`iVBORw0KGgo=` = the PNG signature).
        let raw = b"From: a@x.y\r\nTo: b@x.y\r\nSubject: photo\r\n\
Content-Type: multipart/mixed; boundary=\"B\"\r\n\r\n\
--B\r\nContent-Type: text/plain\r\n\r\nsee attached\r\n\
--B\r\nContent-Type: image/jpeg; name=\"photo.jpg\"\r\n\
Content-Disposition: attachment; filename=\"photo.jpg\"\r\n\
Content-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n\
--B--\r\n";
        let (_, files) = parse_to_new(1, 1, 7, &[], raw, true).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].mime_type.as_deref(), Some("image/png"));
    }
}
