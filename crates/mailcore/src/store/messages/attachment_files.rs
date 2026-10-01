//! Getting attachments out to disk, for both frontends: Save, Save all and
//! the copy a system viewer opens. Every name written comes from
//! [`safe_attachment_name`], since the mail chose it; every target from a
//! dialog goes through [`file_url_to_path`], since dialogs hand back
//! percent-encoded `file://` URLs.
//!
//! The bytes must already be cached (the adapters download first); a row
//! without them fails as [`StoreError::NotFound`].

use std::path::{Path, PathBuf};

use super::attachments::{get_attachment, list_attachments, save_attachment_to_path};
use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::paths::{file_url_to_path, free_path, safe_attachment_name};

/// Write one attachment to a save-dialog target (`file://` URL or path).
/// A directory — existing, or written with a trailing separator — gets the
/// attachment's safe name appended; missing parent folders are created.
/// Returns where the file went.
pub fn save_attachment_to(db: &Db, attachment_id: i64, target: &str) -> Result<PathBuf> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Err(StoreError::InvalidInput("choose where to save".into()));
    }
    let mut dest = file_url_to_path(trimmed);
    if dest.is_dir() || trimmed.ends_with('/') || trimmed.ends_with('\\') {
        let a = get_attachment(db, attachment_id)?;
        dest.push(safe_attachment_name(a.filename.as_deref(), attachment_id));
    }
    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    save_attachment_to_path(db, attachment_id, &dest)?;
    Ok(dest)
}

/// Write every non-inline attachment of a message into `dir` (a dialog
/// value), numbering a name that is already taken (`photo(1).pdf`) instead
/// of overwriting it. Inline parts are the images the reader already shows.
/// A file that fails is logged and skipped; returns how many were saved,
/// and fails when there was nothing to save or nothing could be.
pub fn save_all_attachments_to(db: &Db, message_id: i64, dir: &str) -> Result<u32> {
    let files: Vec<_> = list_attachments(db, message_id)?
        .into_iter()
        .filter(|a| !a.is_inline)
        .collect();
    if files.is_empty() {
        return Err(StoreError::InvalidInput("no attachments to save".into()));
    }
    let dir = file_url_to_path(dir);
    std::fs::create_dir_all(&dir)?;
    let mut saved = 0;
    let mut last_err = None;
    for a in &files {
        let dest = free_path(&dir, &safe_attachment_name(a.filename.as_deref(), a.id));
        match save_attachment_to_path(db, a.id, &dest) {
            Ok(_) => saved += 1,
            Err(e) => {
                log::warn!("save-all: attachment {} failed: {e}", a.id);
                last_err = Some(e);
            }
        }
    }
    match (saved, last_err) {
        (0, Some(e)) => Err(e),
        _ => Ok(saved),
    }
}

/// Write the copy a system viewer opens into `dir` (the platform's temp or
/// cache folder, created when missing) as `<message>-<attachment>-<name>`,
/// so two same-named attachments never replace each other while open.
/// Returns the path.
///
/// Also prunes viewer copies older than a day in the same folder, so
/// opening files does not fill storage over time (notably the app cache
/// on Android). Pruning is best-effort and never fails the open.
pub fn write_attachment_copy(db: &Db, attachment_id: i64, dir: &Path) -> Result<PathBuf> {
    let a = get_attachment(db, attachment_id)?;
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(format!(
        "{}-{}-{}",
        a.message_id,
        attachment_id,
        safe_attachment_name(a.filename.as_deref(), attachment_id)
    ));
    save_attachment_to_path(db, attachment_id, &dest)?;
    crate::paths::prune_temp_copies(dir, Some(&dest));
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount, NewAttachment};
    use crate::store::{accounts, folders, messages};

    fn setup() -> (Db, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &NewAccount {
                name: "a".into(),
                email_address: "a@example.com".into(),
                from_name: String::new(),
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
        let f = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        let mid = messages::upsert(&db, &messages::sample_new(acc, f, 1)).unwrap();
        (db, mid)
    }

    fn attach(db: &Db, mid: i64, name: &str, inline: bool) -> i64 {
        crate::store::messages::add_attachment(
            db,
            mid,
            &NewAttachment {
                filename: Some(name.into()),
                mime_type: Some("application/octet-stream".into()),
                size: 3,
                content_id: None,
                is_inline: inline,
                data: Some(b"abc".to_vec()),
            },
        )
        .unwrap()
    }

    #[test]
    fn save_into_a_folder_uses_the_safe_name() {
        let (db, mid) = setup();
        let id = attach(&db, mid, "../../CON.txt", false);
        let dir = tempfile::tempdir().unwrap();
        let target = format!("{}/", dir.path().display());
        let p = save_attachment_to(&db, id, &target).unwrap();
        assert_eq!(p, dir.path().join("_CON.txt"));
        assert_eq!(std::fs::read(p).unwrap(), b"abc");
        assert!(save_attachment_to(&db, id, "  ").is_err());
    }

    #[test]
    fn save_all_numbers_same_names_and_skips_inline_parts() {
        let (db, mid) = setup();
        attach(&db, mid, "a.txt", false);
        attach(&db, mid, "a.txt", false);
        attach(&db, mid, "logo.png", true);
        let dir = tempfile::tempdir().unwrap();
        let n = save_all_attachments_to(&db, mid, &dir.path().to_string_lossy()).unwrap();
        assert_eq!(n, 2);
        assert!(dir.path().join("a.txt").exists());
        assert!(dir.path().join("a(1).txt").exists());
        assert!(!dir.path().join("logo.png").exists());
    }

    #[test]
    fn save_all_without_files_is_an_error() {
        let (db, mid) = setup();
        attach(&db, mid, "logo.png", true);
        let dir = tempfile::tempdir().unwrap();
        assert!(save_all_attachments_to(&db, mid, &dir.path().to_string_lossy()).is_err());
    }

    #[test]
    fn viewer_copies_of_same_named_files_stay_apart() {
        let (db, mid) = setup();
        let a = attach(&db, mid, "scan.pdf", false);
        let b = attach(&db, mid, "scan.pdf", false);
        let dir = tempfile::tempdir().unwrap();
        let pa = write_attachment_copy(&db, a, &dir.path().join("sub")).unwrap();
        let pb = write_attachment_copy(&db, b, &dir.path().join("sub")).unwrap();
        assert_ne!(pa, pb);
        assert!(pa.ends_with(format!("{mid}-{a}-scan.pdf")));
        assert!(pb.exists());
    }
}
