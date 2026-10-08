//! Server drafts: save (append + replace), open for editing, discard.
//!
//! IMAP has no "edit a message": a replace is an APPEND of the new version
//! plus an expunge of the old. Drafts are destroyed outright on discard,
//! never filed to Trash — discarding an unsent draft means it is gone.

use crate::models::{Folder, FolderRole, Message};
use crate::store::{folders, messages};
use crate::sync::imap::FULL_SYNC_WINDOW;
use crate::sync::pool::{checkout_session, evict_session, job_account};
use crate::sync::sender::{format_draft, SendFormat, SendPolicy};
use crate::Db;
use std::path::{Path, PathBuf};

use super::ComposeForm;

/// The account's Drafts folder, if it has one.
pub fn drafts_folder(db: &Db, account_id: i64) -> Option<Folder> {
    folders::list_by_account(db, account_id)
        .unwrap_or_default()
        .into_iter()
        .find(|f| f.role == FolderRole::Drafts)
}

/// A stored draft, checked to be one: it must sit in a Drafts folder and
/// carry `\Draft`. The adapter turns it into its own composer form.
pub fn open_draft(db: &Db, folder_id: i64, uid: u32) -> Result<Message, String> {
    let folder = folders::get(db, folder_id).map_err(|_| "unknown folder".to_string())?;
    let message = messages::get_by_uid(db, folder_id, uid)
        .map_err(|_| "the draft is no longer available".to_string())?;
    if folder.role != FolderRole::Drafts || !message.is_draft {
        return Err("that message is not a draft".to_string());
    }
    Ok(message)
}

/// A draft's HTML for an editor: sanitized, with its own `cid:` images
/// turned back into `data:` URIs so they show — and go out again inline on
/// the next save or send. Empty when the draft has no HTML part.
pub fn draft_html(db: &Db, message: &Message) -> String {
    let Some(raw) = message
        .body_html
        .as_deref()
        .filter(|h| !h.trim().is_empty())
    else {
        return String::new();
    };
    let clean = crate::html::sanitize_for_send(raw);
    let images = messages::inline_images(db, message.id).unwrap_or_default();
    crate::html::inline_cid_images(&clean, &images).0
}

/// A draft's body for a WYSIWYG editor: its HTML ([`draft_html`]), or the
/// plain text turned into paragraphs when it has no HTML part — set as HTML
/// as is, the text would lose its line breaks.
pub fn draft_editor_html(db: &Db, message: &Message) -> String {
    let html = draft_html(db, message);
    if !html.is_empty() {
        return html;
    }
    match message
        .body_text
        .as_deref()
        .filter(|t| !t.trim().is_empty())
    {
        Some(text) => crate::html::text_to_html(text),
        None => String::new(),
    }
}

/// Result of [`save_draft`]. `previous_not_removed` is set when the new
/// version landed but the one it replaces could not be expunged — untidy
/// (two copies), not destructive.
#[derive(Debug)]
pub struct DraftSaved {
    pub account_id: i64,
    pub drafts_folder_id: i64,
    pub previous_not_removed: Option<String>,
}

/// Append the composer's current text to Drafts, replacing the draft it was
/// opened from (`form.draft_uid`). The Drafts folder is created server-side
/// when the account has none, the way archiving creates Archive.
pub async fn save_draft(
    db: &Db,
    account_id: i64,
    form: &ComposeForm,
) -> Result<DraftSaved, String> {
    let acc = job_account(db, account_id)?;
    let drafts = match drafts_folder(db, acc.id) {
        Some(d) => d,
        None => {
            let delimiter = folders::list_by_account(db, acc.id)
                .unwrap_or_default()
                .first()
                .map(|f| f.delimiter.clone())
                .unwrap_or_else(|| "/".to_string());
            let mut imap = checkout_session(&acc).await?;
            let created = imap
                .create_folder_path(db, acc.id, "Drafts", &delimiter)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            created
        }
    };
    // Check the source still exists before appending, so a stale composer
    // cannot leave two copies behind.
    let source = if form.draft_uid >= 0 {
        let source = messages::get_by_uid(db, drafts.id, form.draft_uid as u32)
            .map_err(|_| "the source draft no longer exists".to_string())?;
        if !source.is_draft {
            return Err("the source message is not a draft".to_string());
        }
        Some(source)
    } else {
        None
    };
    // A draft always keeps its rich text, whatever the user's *send* format
    // preference is — that preference applies to sending.
    let policy = SendPolicy::Unrestricted;
    let raw = format_draft(
        &acc,
        &form.as_request(&acc, SendFormat::Multipart, true, false, &policy),
    )
    .map_err(|e| e.to_string())?;

    let mut imap = checkout_session(&acc).await?;
    imap.append_draft(&drafts.path, &raw)
        .await
        .map_err(|e| e.to_string())?;
    let previous_not_removed = match source {
        Some(source) => imap
            .delete_message(db, source.id)
            .await
            .err()
            .map(|e| e.to_string()),
        None => None,
    };
    imap.sync_folder_window(db, drafts.id, Some(FULL_SYNC_WINDOW), None)
        .await
        .map_err(|e| e.to_string())?;
    imap.checkin();
    if previous_not_removed.is_some() {
        // The failed expunge may have left the session mid-command.
        evict_session(acc.id);
    }
    Ok(DraftSaved {
        account_id: acc.id,
        drafts_folder_id: drafts.id,
        previous_not_removed,
    })
}

/// Destroy a server draft (`\Deleted` + expunge). Returns the Drafts folder
/// id so the adapter can refresh it.
pub async fn delete_draft(db: &Db, account_id: i64, uid: u32) -> Result<i64, String> {
    let acc = job_account(db, account_id)?;
    let drafts =
        drafts_folder(db, acc.id).ok_or_else(|| "the draft is no longer available".to_string())?;
    let m = open_draft(db, drafts.id, uid)?;
    let mut imap = checkout_session(&acc).await?;
    imap.delete_message(db, m.id)
        .await
        .map_err(|e| e.to_string())?;
    imap.checkin();
    Ok(drafts.id)
}

/// One of a draft's own files, written out for the composer to re-attach.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StagedFile {
    /// Plain filesystem path of the staged copy.
    pub path: String,
    /// The name the composer shows (`paths::safe_attachment_name`).
    pub name: String,
}

/// Write each real (non-inline) attachment of a draft into its own fresh
/// staging dir under `base`, so reopening a draft keeps its files: both
/// composers attach from paths. Inline images stay in the body
/// ([`draft_html`]). Fails when a file's bytes are not cached: fetch them
/// first (`sync::attachments::ensure_cached(.., true)`). Stale staging dirs
/// under `base` are pruned on the way.
pub fn stage_draft_files(db: &Db, message_id: i64, base: &Path) -> Result<Vec<StagedFile>, String> {
    let files = messages::list_attachments(db, message_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|a| !a.is_inline)
        .map(|a| {
            Ok(StagedFile {
                path: stage_attachment(db, a.id, base)?
                    .to_string_lossy()
                    .into_owned(),
                name: crate::paths::safe_attachment_name(a.filename.as_deref(), a.id),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    crate::paths::prune_stale_draft_dirs(base);
    Ok(files)
}

/// One cached attachment in a single-use 0700 dir under `base`. The file
/// name keeps the original extension for MIME guessing; the dir per call
/// avoids name collisions in a shared temp folder.
pub(super) fn stage_attachment(
    db: &Db,
    attachment_id: i64,
    base: &Path,
) -> Result<PathBuf, String> {
    use std::io::Write;

    let attachment = messages::get_attachment(db, attachment_id).map_err(|e| e.to_string())?;
    let dir =
        crate::paths::new_stage_dir(base).map_err(|e| format!("cannot create temp folder: {e}"))?;
    let name = format!(
        "{}-{}-{}",
        attachment.message_id,
        attachment.id,
        crate::paths::safe_attachment_name_for_mime(
            attachment.filename.as_deref(),
            attachment.mime_type.as_deref(),
            attachment.id
        )
    );
    let dest = dir.join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)
        .map_err(|e| format!("cannot create draft attachment: {e}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(&dest, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|e| format!("cannot secure draft attachment: {e}"))?;
    let bytes = match attachment.data {
        Some(bytes) if !bytes.is_empty() => bytes,
        _ => {
            let source = attachment
                .storage_path
                .ok_or_else(|| "draft attachment has no cached data".to_string())?;
            std::fs::read(source).map_err(|e| format!("cannot read draft attachment: {e}"))?
        }
    };
    file.write_all(&bytes)
        .map_err(|e| format!("cannot write draft attachment: {e}"))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAccount;
    use crate::store::accounts;

    fn setup() -> (Db, i64) {
        let db = Db::open_in_memory().unwrap();
        let id = accounts::create(
            &db,
            &NewAccount {
                name: "t".to_string(),
                email_address: "me@example.com".to_string(),
                from_name: String::new(),
                imap_host: "imap.example.com".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "smtp.example.com".to_string(),
                smtp_port: 587,
                smtp_security: "starttls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 300,
            },
        )
        .unwrap();
        (db, id)
    }

    #[test]
    fn open_draft_requires_a_drafts_folder_and_the_draft_flag() {
        let (db, acc) = setup();
        let drafts = folders::upsert(&db, acc, "Drafts", "/", FolderRole::Drafts).unwrap();
        let inbox = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        assert_eq!(drafts_folder(&db, acc).map(|f| f.id), Some(drafts));

        let mut d = messages::sample_new(acc, drafts, 1);
        d.is_draft = true;
        messages::upsert(&db, &d).unwrap();
        let mut plain = messages::sample_new(acc, drafts, 2);
        plain.is_draft = false;
        messages::upsert(&db, &plain).unwrap();
        let mut elsewhere = messages::sample_new(acc, inbox, 3);
        elsewhere.is_draft = true;
        messages::upsert(&db, &elsewhere).unwrap();

        assert_eq!(open_draft(&db, drafts, 1).unwrap().uid, 1);
        assert!(open_draft(&db, drafts, 2).is_err());
        assert!(open_draft(&db, inbox, 3).is_err());
        assert!(open_draft(&db, drafts, 99).is_err());
    }

    #[test]
    fn editor_body_is_the_html_part_or_the_text_as_paragraphs() {
        let (db, acc) = setup();
        let drafts = folders::upsert(&db, acc, "Drafts", "/", FolderRole::Drafts).unwrap();
        let mut d = messages::sample_new(acc, drafts, 1);
        d.is_draft = true;
        d.body_text = Some(
            "one

two"
            .to_string(),
        );
        d.body_html = None;
        messages::upsert(&db, &d).unwrap();
        let m = open_draft(&db, drafts, 1).unwrap();
        assert_eq!(
            draft_editor_html(&db, &m),
            crate::html::text_to_html(
                "one

two"
            )
        );

        d.body_html = Some("<p><b>bold</b></p>".to_string());
        messages::upsert(&db, &d).unwrap();
        let m = open_draft(&db, drafts, 1).unwrap();
        assert_eq!(draft_editor_html(&db, &m), "<p><b>bold</b></p>");
    }

    #[test]
    fn stage_draft_files_writes_real_attachments_only() {
        use crate::models::NewAttachment;
        let (db, acc) = setup();
        let drafts = folders::upsert(&db, acc, "Drafts", "/", FolderRole::Drafts).unwrap();
        let mut d = messages::sample_new(acc, drafts, 1);
        d.is_draft = true;
        let id = messages::upsert(&db, &d).unwrap();
        let file = |name: &str, inline: bool, data: Option<&[u8]>| NewAttachment {
            filename: Some(name.to_string()),
            mime_type: Some("text/plain".to_string()),
            content_id: inline.then(|| "c1".to_string()),
            size: 4,
            data: data.map(<[u8]>::to_vec),
            is_inline: inline,
        };
        messages::add_attachment(&db, id, &file("notes.txt", false, Some(b"abcd"))).unwrap();
        messages::add_attachment(&db, id, &file("logo.png", true, Some(b"png!"))).unwrap();
        let base = tempfile::tempdir().unwrap();

        let staged = stage_draft_files(&db, id, base.path()).unwrap();
        assert_eq!(staged.len(), 1, "inline images stay in the body");
        assert_eq!(staged[0].name, "notes.txt");
        assert_eq!(std::fs::read(&staged[0].path).unwrap(), b"abcd");

        messages::add_attachment(&db, id, &file("later.txt", false, None)).unwrap();
        assert!(
            stage_draft_files(&db, id, base.path()).is_err(),
            "uncached bytes must be fetched first"
        );
    }
}
