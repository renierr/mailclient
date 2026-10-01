use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::compose::{self, ComposeForm};

use crate::bridge::messages::{draft_attachment_path, ensure_attachment_data};
use crate::bridge::qobject;
use crate::bridge::qstring;
use crate::bridge::worker::{spawn_job, JobRefresh, BUSY_MESSAGE};

// Thin adapter over `mailcore::compose`: parse the QML form, start the job,
// phrase the result. The send/draft rules themselves live in the core.

impl qobject::Bridge {
    pub fn send_mail(self: Pin<&mut Self>, form: &QString) -> QString {
        let form = match ComposeForm::parse(&form.to_string()) {
            Ok(f) => f,
            Err(e) => return qstring(&e),
        };
        // Refuse while busy *before* the outbox row exists: `spawn_job` would
        // refuse too, but by then the message would sit queued and go out
        // with the next sync behind a "busy" the user reads as "not sent".
        if *self.busy() {
            return qstring(BUSY_MESSAGE);
        }
        let wanted = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        // Validate + build + enqueue synchronously, so mistakes report
        // instantly with the composer still open.
        let db = match crate::bridge::shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let prepared = match compose::prepare_send(db, wanted, form) {
            Ok(p) => p,
            Err(e) => return qstring(&e),
        };
        let queue_id = prepared.queue_id;
        let queued = spawn_job(self, "Send", move |db, progress| async move {
            // SMTP accepted it: release the composer now rather than holding
            // it open through the Sent copy, the draft removal and the resync.
            let outcome = compose::deliver(db, prepared, folder_id, || progress.report("")).await?;
            Ok((
                outcome.notes.join(" "),
                Some(JobRefresh::feeds(outcome.account_id, folder_id)),
            ))
        });
        if !queued.is_empty() {
            // Not started after all (unreachable while the busy check above
            // holds, since both run on the GUI thread) — never leave an
            // orphan row behind for crash recovery to deliver later.
            let _ = mailcore::store::queue::discard_mime(db, queue_id);
        }
        queued
    }

    pub fn save_draft(self: Pin<&mut Self>, form: &QString) -> QString {
        let form = match ComposeForm::parse(&form.to_string()) {
            Ok(f) => f,
            Err(_) => return qstring("invalid draft form"),
        };
        let wanted = *self.current_account_id();
        let current_folder_id = *self.current_folder_id();
        spawn_job(self, "Save draft", move |db, _progress| async move {
            let saved = compose::save_draft(db, wanted, &form).await?;
            if let Some(e) = saved.previous_not_removed {
                return Err(format!(
                    "draft saved, but could not remove source draft: {e}"
                ));
            }
            Ok((
                String::new(),
                Some(JobRefresh::feeds(saved.account_id, current_folder_id)),
            ))
        })
    }

    pub fn image_data_url(&self, path: &QString) -> QString {
        match compose::image_data_url(&path.to_string()) {
            Ok(url) => qstring(&url),
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn is_inline_image(&self, path: &QString) -> bool {
        compose::is_inline_image_file(&path.to_string())
    }

    pub fn draft_form(self: Pin<&mut Self>, uid: i32) -> QString {
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Open draft", move |db, _progress| async move {
            let message = compose::open_draft(db, folder_id, uid as u32)?;
            ensure_attachment_data(db, message.id, true).await?;
            // QML's FileDialog deals in paths, so the draft's files are
            // materialized as temp copies the composer can re-attach.
            // Inline images come back inside the body (see `draft_html`),
            // so only real attachments are re-attached as files.
            let attachments = mailcore::store::messages::list_attachments(db, message.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|a| !a.is_inline)
                .map(|a| {
                    let path = draft_attachment_path(db, a.id)?;
                    Ok(serde_json::json!({
                        "path": path,
                        "name": mailcore::paths::safe_attachment_name(a.filename.as_deref(), a.id),
                    }))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let body_html = compose::draft_html(db, &message);
            Ok((
                serde_json::json!({
                    "draft_uid": message.uid,
                    "from": message.from_addr.unwrap_or_default(),
                    "reply_to": message.reply_to.unwrap_or_default(),
                    "to": message.to_addrs.join(", "),
                    "cc": message.cc_addrs.join(", "),
                    "bcc": message.bcc_addrs.join(", "),
                    "subject": message.subject.unwrap_or_default(),
                    "body": Some(body_html)
                        .filter(|h| !h.is_empty())
                        .or(message.body_text)
                        .unwrap_or_default(),
                    "attachments": attachments,
                })
                .to_string(),
                // Read-only: opening a draft must not rebuild the feed under
                // the list the user just clicked in.
                None,
            ))
        })
    }

    pub fn delete_draft(self: Pin<&mut Self>, uid: i32) -> QString {
        if uid < 0 {
            return qstring("draft is no longer available");
        }
        let wanted = *self.current_account_id();
        let current = *self.current_folder_id();
        spawn_job(self, "Delete", move |db, _progress| async move {
            compose::delete_draft(db, wanted, uid as u32).await?;
            Ok((
                "Draft deleted".to_string(),
                Some(JobRefresh::feeds(wanted, current)),
            ))
        })
    }
}
