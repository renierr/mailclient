//! Sending and drafts — a thin adapter over `mailcore::compose`, which owns
//! the rules; this file only starts the jobs and phrases their results.

use mailcore::compose::{self, ComposeForm};

use crate::db::shared_db;
use crate::net::{spawn, JobRefresh};

/// Send a message from the composer's JSON form
/// (`{to, cc?, bcc?, from?, from_name?, reply_to?, subject, body, body_html?,
/// attachments?, draft_uid?}`).
///
/// Validation, MIME assembly and queueing happen **inline**, before this
/// returns, so a mistake comes back instantly with the composer still open.
/// Only the SMTP submit is queued, which is why the composer can close on the
/// `Send` job's *progress* event instead of waiting for the Sent copy and the
/// resync that follow it.
pub fn send_mail(account_id: i64, folder_id: i64, form: String) -> anyhow::Result<()> {
    let form = ComposeForm::parse(&form).map_err(anyhow::Error::msg)?;
    let db = shared_db()?;
    let prepared = compose::prepare_send(db, account_id, form).map_err(anyhow::Error::msg)?;
    let queue_id = prepared.queue_id;
    let started = spawn(
        "Send",
        format!("send:{queue_id}"),
        move |db, progress| async move {
            let outcome = compose::deliver(db, prepared, folder_id, || progress.report("")).await?;
            let status = if outcome.notes.is_empty() {
                "Sent".to_string()
            } else {
                outcome.notes.join("; ")
            };
            Ok((
                status,
                Some(JobRefresh::folder(outcome.account_id, folder_id)),
            ))
        },
    );
    if started.is_err() {
        // Never leave a claimed row behind for crash recovery to deliver.
        let _ = mailcore::store::queue::discard_mime(db, queue_id);
    }
    started
}

/// Append the composer's current text to Drafts, replacing what it was
/// opened from. The Drafts folder is created server-side when missing.
pub fn save_draft(account_id: i64, form: String) -> anyhow::Result<()> {
    let form = ComposeForm::parse(&form).map_err(anyhow::Error::msg)?;
    spawn(
        "Save draft",
        format!("draft:{account_id}"),
        move |db, _progress| async move {
            let saved = compose::save_draft(db, account_id, &form).await?;
            let note = saved
                .previous_not_removed
                .map(|e| format!(" (the previous version could not be removed: {e})"))
                .unwrap_or_default();
            Ok((
                format!("Draft saved{note}"),
                Some(JobRefresh::folder(saved.account_id, saved.drafts_folder_id)),
            ))
        },
    )
}

/// The composer form for a stored draft, so editing can continue.
///
/// Attachments come back as metadata, never as file paths. The Qt frontend
/// materializes them into temp files because its FileDialog deals in paths;
/// Flutter does not need that, and on Android there is nowhere sane to put
/// them — the composer holds attachment ids and the bytes are fetched through
/// [`crate::api::attachments`] on demand.
pub fn draft_form(account_id: i64, uid: u32) -> anyhow::Result<String> {
    let db = shared_db()?;
    let drafts = compose::drafts_folder(db, account_id)
        .ok_or_else(|| anyhow::anyhow!("this account has no Drafts folder"))?;
    let m = compose::open_draft(db, drafts.id, uid).map_err(anyhow::Error::msg)?;
    // The composer edits addresses as one line per field.
    let line = |addrs: &[String]| addrs.join(", ");
    let attachments: serde_json::Value =
        serde_json::from_str(&mailcore::feed::attachments_json(db, drafts.id, uid)?)
            .unwrap_or_else(|_| serde_json::Value::Array(Vec::new()));
    let body_html = compose::draft_html(db, &m);
    Ok(serde_json::json!({
        "draft_uid": uid,
        "from": m.from_addr.unwrap_or_default(),
        "to": line(&m.to_addrs),
        "cc": line(&m.cc_addrs),
        "bcc": line(&m.bcc_addrs),
        "reply_to": m.reply_to.unwrap_or_default(),
        "subject": m.subject.unwrap_or_default(),
        "body": m.body_text.unwrap_or_default(),
        "body_html": body_html,
        "attachments": attachments,
    })
    .to_string())
}

/// An image file as a `data:` URL for the composer to show inline; the
/// sender turns it into a `cid:` part. Errors for non-image types and for
/// images too large to go inline (attach those instead).
pub fn image_data_url(path: String) -> anyhow::Result<String> {
    compose::image_data_url(&path).map_err(|e| anyhow::anyhow!(e.to_string()))
}

/// Whether a file would be offered as an inline image (by type).
#[flutter_rust_bridge::frb(sync)]
pub fn is_inline_image(path: String) -> bool {
    compose::is_inline_image_file(&path)
}

/// Destroy a server draft (`\Deleted` + expunge, never filed to Trash) —
/// what Discard means for a draft opened from the Drafts folder.
pub fn delete_draft(account_id: i64, uid: u32) -> anyhow::Result<()> {
    let key = format!("draft-del:{account_id}:{uid}");
    spawn("Delete draft", key, move |db, _progress| async move {
        let drafts_id = compose::delete_draft(db, account_id, uid).await?;
        Ok((
            "Draft discarded".to_string(),
            Some(JobRefresh::folder(account_id, drafts_id)),
        ))
    })
}
