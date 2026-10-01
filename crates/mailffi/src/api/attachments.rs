//! Attachments.
//!
//! Sync stores names and sizes only (inline images aside, which the reader
//! embeds); bytes cost bandwidth and are downloaded on explicit request, then cached in SQLite as BLOBs. Every function here
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
use mailcore::sync::pool::{checkout_session, resolve_account};

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

/// Write one cached attachment to `path` (desktop "Save as…"), a file or
/// a folder (which gets the attachment's safe name). Returns where it went.
///
/// Fails rather than downloading when the bytes are not cached yet: a save
/// dialog has already been through, and silently turning it into a network
/// wait is the kind of surprise a progress event exists to avoid. Naming and
/// writing are `mailcore`'s, shared with the Qt adapter.
pub fn save_attachment_to(attachment_id: i64, path: String) -> anyhow::Result<String> {
    let dest = messages::save_attachment_to(shared_db()?, attachment_id, &path)?;
    Ok(dest.to_string_lossy().into_owned())
}

/// Write every non-inline attachment of a message into `dir`, numbering
/// names that are already taken. Returns how many were saved.
pub fn save_all_attachments_to(folder_id: i64, uid: u32, dir: String) -> anyhow::Result<u64> {
    let db = shared_db()?;
    let m = messages::get_by_uid(db, folder_id, uid)?;
    Ok(messages::save_all_attachments_to(db, m.id, &dir)?.into())
}

/// Write the copy a system viewer opens into `dir` (the app's temp or cache
/// folder) under a name that cannot clash or escape it. Returns the path.
pub fn write_attachment_copy(attachment_id: i64, dir: String) -> anyhow::Result<String> {
    let dest =
        messages::write_attachment_copy(shared_db()?, attachment_id, std::path::Path::new(&dir))?;
    Ok(dest.to_string_lossy().into_owned())
}
