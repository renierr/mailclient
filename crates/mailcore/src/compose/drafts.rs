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
    imap.sync_folder_window(db, drafts.id, Some(FULL_SYNC_WINDOW))
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
}
