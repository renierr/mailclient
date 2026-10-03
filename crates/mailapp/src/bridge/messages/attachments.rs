//! Materializing one cached attachment file for the composer. The
//! on-demand download itself is `mailcore::sync::attachments`.

use mailcore::store::messages;

use mailcore::paths::safe_attachment_name_for_mime;

use super::files::file_url;

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
        safe_attachment_name_for_mime(
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
    drop(file);
    // Each draft staging dir is single-use; prune the ones older than a day
    // so abandoned drafts do not fill the temp folder. Best-effort.
    mailcore::paths::prune_stale_draft_dirs(&base);
    Ok(file_url(&dest))
}
