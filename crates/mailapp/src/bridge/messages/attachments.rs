//! Getting attachment bytes: the on-demand download, and materializing one
//! file for the composer.

use crate::bridge::session::checkout_session;
use mailcore::store::{accounts, folders, messages};

use super::files::{file_url, safe_filename};

/// Make sure a message's file bytes are cached, downloading them now on
/// explicit user request. Background sync stores names/sizes only, so this
/// is the single place attachment bytes cross the network. Returns the
/// number of files downloaded (0 = already cached). Draft opening includes
/// inline parts because Composer must preserve them on replacement.
pub(crate) async fn ensure_attachment_data(
    db: &mailcore::Db,
    message_id: i64,
    include_inline: bool,
) -> Result<u64, String> {
    let files = messages::list_attachments(db, message_id).map_err(|e| e.to_string())?;
    let mut missing = false;
    for a in files.iter().filter(|a| include_inline || !a.is_inline) {
        let has = messages::attachment_has_data(db, a.id).map_err(|e| e.to_string())?;
        if !has {
            missing = true;
            break;
        }
    }
    if !missing {
        return Ok(0);
    }
    let msg = messages::get(db, message_id).map_err(|e| e.to_string())?;
    let folder = folders::get(db, msg.folder_id).map_err(|e| e.to_string())?;
    let acc = accounts::get(db, folder.account_id).map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    let mut imap = checkout_session(&acc).await?;
    let n = imap
        .fetch_attachments(db, message_id)
        .await
        .map_err(|e| e.to_string())?;
    imap.checkin();
    log::info!(
        "attachments: downloaded {n} file(s) for message {message_id} in {:?}",
        started.elapsed()
    );
    Ok(n)
}

/// Materialize one cached attachment for Composer. The file name keeps the
/// original extension for MIME guessing. Each invocation owns a 0700 temp
/// directory, avoiding shared-temp name collisions.
pub(crate) fn draft_attachment_path(
    db: &mailcore::Db,
    attachment_id: i64,
) -> Result<String, String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);
    let attachment = messages::get_attachment(db, attachment_id).map_err(|e| e.to_string())?;
    let base = std::env::temp_dir();
    let unique = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let dir = base.join(format!("mailclient-draft-{}-{unique}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| format!("cannot create temp folder: {e}"))?;
    #[cfg(unix)]
    std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|e| format!("cannot secure temp folder: {e}"))?;
    let name = format!(
        "{}-{}-{}",
        attachment.message_id,
        attachment.id,
        safe_filename(attachment.filename.as_deref(), attachment.id)
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
    Ok(file_url(&dest))
}
