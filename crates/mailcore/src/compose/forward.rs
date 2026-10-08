//! Forwarding keeps the original's files: they are fetched if needed and
//! staged for the composer, which attaches from paths like any picked file.
//! An edit-and-resend of a bounced mail does the same with the sent
//! original the bounce reports on.

use crate::store::messages;
use crate::Db;
use std::path::Path;

use super::drafts::stage_attachment;
use super::StagedFile;

/// The files a forward starts with.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ForwardFiles {
    /// Real (non-inline) attachments staged for the composer. Inline images
    /// travel in the quoted body instead.
    pub files: Vec<StagedFile>,
    /// Names of files whose bytes could not be fetched (offline, server
    /// error): the forward goes without them.
    pub missing: Vec<String>,
    /// What to tell the user about `missing`; empty when nothing is missing.
    pub notice: String,
}

/// How many of the original's real attachments have no cached bytes: when
/// non-zero, an adapter fetches them before [`stage_forward_files`] (or runs
/// [`forward_files`], which does both). Local read.
pub fn forward_missing(db: &Db, folder_id: i64, uid: u32) -> Result<usize, String> {
    let m = original(db, folder_id, uid)?;
    crate::sync::attachments::missing_count(db, m.id, false).map_err(|e| e.to_string())
}

/// Fetch the original's files when they are not cached yet (no network when
/// they are), then stage them under `base`. A failed fetch is not fatal:
/// whatever is cached is staged and the rest is named in `missing`.
pub async fn forward_files(
    db: &Db,
    folder_id: i64,
    uid: u32,
    base: &Path,
) -> Result<ForwardFiles, String> {
    let m = original(db, folder_id, uid)?;
    if let Err(e) = crate::sync::attachments::ensure_cached(db, m.id, false).await {
        log::warn!("forward: attachments of message {} not fetched: {e}", m.id);
    }
    stage_message_files(db, m.id, base)
}

/// Stage what is cached of the original's real attachments under `base`
/// and name the rest. Local only, never fetches.
pub fn stage_forward_files(
    db: &Db,
    folder_id: i64,
    uid: u32,
    base: &Path,
) -> Result<ForwardFiles, String> {
    stage_message_files(db, original(db, folder_id, uid)?.id, base)
}

/// [`forward_missing`] for an edit-and-resend: counts the files of the sent
/// original the bounce at `(folder_id, uid)` reports on.
pub fn resend_missing(db: &Db, folder_id: i64, uid: u32) -> Result<usize, String> {
    let m = crate::report::bounce(db, folder_id, uid)?.original;
    crate::sync::attachments::missing_count(db, m.id, false).map_err(|e| e.to_string())
}

/// [`forward_files`] for an edit-and-resend: fetch and stage the files of
/// the bounce's sent original.
pub async fn resend_files(
    db: &Db,
    folder_id: i64,
    uid: u32,
    base: &Path,
) -> Result<ForwardFiles, String> {
    let m = crate::report::bounce(db, folder_id, uid)?.original;
    forward_files(db, m.folder_id, m.uid, base).await
}

/// [`stage_forward_files`] for an edit-and-resend: stage what is cached of
/// the bounce's sent original. Local only.
pub fn stage_resend_files(
    db: &Db,
    folder_id: i64,
    uid: u32,
    base: &Path,
) -> Result<ForwardFiles, String> {
    let m = crate::report::bounce(db, folder_id, uid)?.original;
    stage_message_files(db, m.id, base)
}

fn original(db: &Db, folder_id: i64, uid: u32) -> Result<crate::models::Message, String> {
    messages::get_by_uid(db, folder_id, uid)
        .map_err(|_| "this message is no longer available".to_string())
}

fn stage_message_files(db: &Db, message_id: i64, base: &Path) -> Result<ForwardFiles, String> {
    let mut out = ForwardFiles::default();
    for a in messages::list_attachments(db, message_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|a| !a.is_inline)
    {
        let name = crate::paths::safe_attachment_name(a.filename.as_deref(), a.id);
        let cached = messages::attachment_has_data(db, a.id).map_err(|e| e.to_string())?;
        match cached.then(|| stage_attachment(db, a.id, base)) {
            Some(Ok(path)) => out.files.push(StagedFile {
                path: path.to_string_lossy().into_owned(),
                name,
            }),
            Some(Err(e)) => {
                log::warn!("forward: attachment {} not staged: {e}", a.id);
                out.missing.push(name);
            }
            None => out.missing.push(name),
        }
    }
    crate::paths::prune_stale_draft_dirs(base);
    out.notice = missing_notice(&out.missing);
    Ok(out)
}

fn missing_notice(missing: &[String]) -> String {
    match missing.len() {
        0 => String::new(),
        1 => format!(
            "“{}” could not be downloaded and is not attached.",
            missing[0]
        ),
        n => format!(
            "{n} files could not be downloaded and are not attached: {}.",
            missing.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FolderRole, NewAccount, NewAttachment};
    use crate::store::{accounts, folders};

    fn setup() -> (Db, i64) {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
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
        (db, acc)
    }

    fn file(name: &str, inline: bool, data: Option<&[u8]>) -> NewAttachment {
        NewAttachment {
            filename: Some(name.to_string()),
            mime_type: Some("text/plain".to_string()),
            content_id: inline.then(|| "c1".to_string()),
            size: 4,
            data: data.map(<[u8]>::to_vec),
            is_inline: inline,
        }
    }

    #[test]
    fn stages_cached_files_and_names_the_rest() {
        let (db, acc) = setup();
        let inbox = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        let id = messages::upsert(&db, &messages::sample_new(acc, inbox, 1)).unwrap();
        messages::add_attachment(&db, id, &file("report.txt", false, Some(b"abcd"))).unwrap();
        messages::add_attachment(&db, id, &file("logo.png", true, Some(b"png!"))).unwrap();
        messages::add_attachment(&db, id, &file("big.zip", false, None)).unwrap();
        let base = tempfile::tempdir().unwrap();

        assert_eq!(forward_missing(&db, inbox, 1).unwrap(), 1);
        let out = stage_forward_files(&db, inbox, 1, base.path()).unwrap();
        assert_eq!(out.files.len(), 1, "inline images stay in the quoted body");
        assert_eq!(out.files[0].name, "report.txt");
        assert_eq!(std::fs::read(&out.files[0].path).unwrap(), b"abcd");
        assert_eq!(out.missing, vec!["big.zip".to_string()]);
        assert!(out.notice.contains("big.zip"));
    }

    #[test]
    fn nothing_missing_means_no_notice() {
        let (db, acc) = setup();
        let inbox = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        messages::upsert(&db, &messages::sample_new(acc, inbox, 1)).unwrap();
        let base = tempfile::tempdir().unwrap();
        assert_eq!(forward_missing(&db, inbox, 1).unwrap(), 0);
        assert!(stage_forward_files(&db, inbox, 2, base.path()).is_err());
        assert_eq!(
            stage_forward_files(&db, inbox, 1, base.path()).unwrap(),
            ForwardFiles::default()
        );
        assert_eq!(
            missing_notice(&["a".into(), "b".into()]),
            "2 files could not be downloaded and are not attached: a, b."
        );
    }
}
