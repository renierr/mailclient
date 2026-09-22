//! Attachments.
//!
//! Sync stores names and sizes only; bytes cost bandwidth and are downloaded
//! on explicit request, then cached in SQLite as BLOBs. Every function here
//! is therefore a deliberate user action, never something a list render does.
//!
//! Bytes cross the FFI boundary as a `Vec<u8>` rather than as a file path.
//! The Qt frontend writes a temp file and hands QML a `file://` URL because
//! `Qt.openUrlExternally` needs one; Flutter's share/save/preview plugins
//! take bytes, and Android has no path a user could open anyway. Saving to a
//! location the user picked is then Dart's job, through the platform's own
//! file picker — which is also the only way it can work under scoped storage.

use mailcore::store::messages;

use crate::db::shared_db;
use crate::net::spawn;
use crate::session::{checkout_session, resolve_account};

/// One attachment's bytes, if they are already cached.
///
/// `None` means "not downloaded yet" — call [`download_attachments`] and try
/// again once its job finishes. This never touches the network, so it is safe
/// to call while rendering.
pub fn cached_attachment_bytes(attachment_id: i64) -> anyhow::Result<Option<Vec<u8>>> {
    let db = shared_db()?;
    if !messages::attachment_has_data(db, attachment_id)? {
        return Ok(None);
    }
    Ok(messages::get_attachment(db, attachment_id)?.data)
}

/// Download every attachment of one message into the local cache.
///
/// Whole-message rather than per-file because IMAP fetches by body part
/// within one FETCH: pulling three files one at a time would be three round
/// trips for the same bytes. Finishing is reported as an `"Attachments"`
/// event, after which [`cached_attachment_bytes`] answers.
pub fn download_attachments(account_id: i64, folder_id: i64, uid: u32) -> anyhow::Result<()> {
    let key = format!("attach:{folder_id}:{uid}");
    spawn("Attachments", key, move |db, _progress| async move {
        let acc = resolve_account(db, account_id)?;
        let m = messages::get_by_uid(db, folder_id, uid).map_err(|e| e.to_string())?;
        if m.account_id != acc.id {
            return Err("message does not belong to this account".to_string());
        }
        let mut imap = checkout_session(&acc).await?;
        let result = imap.fetch_attachments(db, m.id).await;
        imap.checkin();
        let bytes = result.map_err(|e| e.to_string())?;
        // Nothing the message list shows changed, so no refresh — the reader
        // re-reads the one message it is showing.
        Ok((format!("Downloaded {bytes} bytes"), None))
    })
}

/// Write one cached attachment to `path` (desktop "Save as…").
///
/// A directory target appends the attachment's own filename. Fails rather
/// than downloading when the bytes are not cached yet: a save dialog has
/// already been through, and silently turning it into a network wait is the
/// kind of surprise a progress event exists to avoid.
pub fn save_attachment_to(attachment_id: i64, path: String) -> anyhow::Result<String> {
    let db = shared_db()?;
    let a = messages::get_attachment(db, attachment_id)?;
    let mut dest = std::path::PathBuf::from(strip_file_url(&path));
    if dest.is_dir() {
        dest.push(safe_filename(a.filename.as_deref().unwrap_or_default()));
    }
    messages::save_attachment_to_path(db, attachment_id, &dest)?;
    Ok(dest.to_string_lossy().into_owned())
}

/// Write every non-inline attachment of a message into `dir`.
pub fn save_all_attachments_to(folder_id: i64, uid: u32, dir: String) -> anyhow::Result<u64> {
    let db = shared_db()?;
    let m = messages::get_by_uid(db, folder_id, uid)?;
    let dir = std::path::PathBuf::from(strip_file_url(&dir));
    let mut saved = 0;
    for a in messages::list_attachments(db, m.id)? {
        // Inline parts are the images the reader already shows; writing them
        // next to the real files is noise.
        if a.is_inline {
            continue;
        }
        let name = safe_filename(a.filename.as_deref().unwrap_or_default());
        messages::save_attachment_to_path(db, a.id, &dir.join(name))?;
        saved += 1;
    }
    Ok(saved)
}

/// Strip a `file://` prefix from a path handed over by a file picker.
fn strip_file_url(path: &str) -> String {
    let rest = match path.strip_prefix("file://") {
        Some(r) => r,
        None => return path.to_string(),
    };
    // Windows URLs are `file:///C:/…`; the leading slash is not part of the
    // path there, but on Unix it is the root and must stay.
    match rest.strip_prefix('/') {
        Some(win) if win.chars().nth(1) == Some(':') => win.to_string(),
        _ => rest.to_string(),
    }
}

/// A filename safe to join onto a directory the user picked.
///
/// The name comes from the message, which is to say from a stranger: a part
/// called `../../.bashrc` must land as a file in the chosen folder, not
/// somewhere else entirely.
fn safe_filename(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("attachment")
        .trim();
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    match cleaned.trim_matches('.') {
        "" => "attachment".to_string(),
        s => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{safe_filename, strip_file_url};

    #[test]
    fn a_traversing_attachment_name_stays_inside_the_chosen_folder() {
        assert_eq!(safe_filename("../../.bashrc"), "bashrc");
        assert_eq!(safe_filename("C:\\windows\\system32\\evil.dll"), "evil.dll");
        assert_eq!(safe_filename("   "), "attachment");
        assert_eq!(safe_filename("report:2024?.pdf"), "report_2024_.pdf");
        assert_eq!(safe_filename("normal name.pdf"), "normal name.pdf");
    }

    #[test]
    fn file_urls_lose_their_scheme_without_losing_the_unix_root() {
        assert_eq!(strip_file_url("file:///home/me/x.pdf"), "/home/me/x.pdf");
        assert_eq!(
            strip_file_url("file:///C:/Users/me/x.pdf"),
            "C:/Users/me/x.pdf"
        );
        assert_eq!(strip_file_url("/plain/path"), "/plain/path");
    }
}
