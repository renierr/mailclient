use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::compose::{self, ComposeForm};

use crate::bridge::messages::file_url;
use crate::bridge::qobject;
use crate::bridge::worker::{spawn_job, JobDone, JobRefresh, BUSY_MESSAGE};
use crate::bridge::{qstring, shared_db};
use mailcore::sync::attachments::ensure_cached;

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
            Ok(JobDone {
                status: outcome.notes.join(" "),
                refresh: Some(JobRefresh::feeds(outcome.account_id, folder_id)),
                outcome: outcome.outcome().to_string(),
            })
        });
        if !queued.is_empty() {
            // Not started after all (unreachable while the busy check above
            // holds, since both run on the GUI thread) — drop the row
            // entirely: the composer is still open with the text intact, and
            // a kept row would sit in the outbox list forever with no bytes.
            mailcore::compose::abandon_send(db, queue_id);
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

    pub fn sender_parts_json(&self, address: &QString) -> QString {
        let parts = compose::sender_parts(&address.to_string());
        qstring(&serde_json::to_string(&parts).unwrap_or_default())
    }

    pub fn effective_from(&self, local: &QString, account_email: &QString) -> QString {
        qstring(&compose::effective_from(
            &local.to_string(),
            &account_email.to_string(),
        ))
    }

    pub fn send_format_note(&self, format: &QString, html: &QString) -> QString {
        qstring(compose::editor::send_format_note(
            &format.to_string(),
            &html.to_string(),
        ))
    }

    pub fn forward_missing(&self, uid: i32) -> i32 {
        let Ok(db) = shared_db() else {
            return 0;
        };
        compose::forward_missing(db, *self.current_folder_id(), uid.max(0) as u32)
            .map_or(0, |n| i32::try_from(n).unwrap_or(i32::MAX))
    }

    pub fn forward_draft_json(&self, uid: i32) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("{}");
        };
        let folder_id = *self.current_folder_id();
        let uid = uid.max(0) as u32;
        compose::stage_forward_files(db, folder_id, uid, &std::env::temp_dir())
            .and_then(|files| answer_payload(db, folder_id, uid, "forward", files))
            .map_or_else(|_| qstring("{}"), |j| qstring(&j))
    }

    pub fn resend_missing(&self, uid: i32) -> i32 {
        let Ok(db) = shared_db() else {
            return 0;
        };
        compose::resend_missing(db, *self.current_folder_id(), uid.max(0) as u32)
            .map_or(0, |n| i32::try_from(n).unwrap_or(i32::MAX))
    }

    pub fn resend_draft_json(&self, uid: i32) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("{}");
        };
        let folder_id = *self.current_folder_id();
        let uid = uid.max(0) as u32;
        compose::stage_resend_files(db, folder_id, uid, &std::env::temp_dir())
            .and_then(|files| answer_payload(db, folder_id, uid, "resend", files))
            .map_or_else(|_| qstring("{}"), |j| qstring(&j))
    }

    pub fn resend_fetch(self: Pin<&mut Self>, uid: i32) -> QString {
        let folder_id = *self.current_folder_id();
        let uid = uid.max(0) as u32;
        spawn_job(self, "Resend", move |db, _progress| async move {
            let files = compose::resend_files(db, folder_id, uid, &std::env::temp_dir()).await?;
            // Read-only: nothing in the feeds changed.
            Ok((answer_payload(db, folder_id, uid, "resend", files)?, None))
        })
    }

    pub fn forward_fetch(self: Pin<&mut Self>, uid: i32) -> QString {
        let folder_id = *self.current_folder_id();
        let uid = uid.max(0) as u32;
        spawn_job(self, "Forward", move |db, _progress| async move {
            let files = compose::forward_files(db, folder_id, uid, &std::env::temp_dir()).await?;
            // Read-only: nothing in the feeds changed.
            Ok((answer_payload(db, folder_id, uid, "forward", files)?, None))
        })
    }

    pub fn draft_form(self: Pin<&mut Self>, uid: i32) -> QString {
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Open draft", move |db, _progress| async move {
            let message = compose::open_draft(db, folder_id, uid as u32)?;
            ensure_cached(db, message.id, true).await?;
            // The draft's own files as temp copies the composer re-attaches
            // (`compose::stage_draft_files`); inline images stay in the body.
            let attachments: Vec<_> =
                compose::stage_draft_files(db, message.id, &std::env::temp_dir())?
                    .into_iter()
                    .map(|f| {
                        serde_json::json!({
                            "path": file_url(std::path::Path::new(&f.path)),
                            "name": f.name,
                        })
                    })
                    .collect();
            // The editor takes HTML: a plain draft becomes paragraphs, not
            // one run-together line.
            let body_html = compose::draft_editor_html(db, &message);
            Ok((
                serde_json::json!({
                    "draft_uid": message.uid,
                    "from": message.from_addr.unwrap_or_default(),
                    "reply_to": message.reply_to.unwrap_or_default(),
                    "to": message.to_addrs.join(", "),
                    "cc": message.cc_addrs.join(", "),
                    "bcc": message.bcc_addrs.join(", "),
                    "subject": message.subject.unwrap_or_default(),
                    "body": body_html,
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

/// The forward or resend answer draft with the staged files merged in, the
/// way the QML composer takes a draft (`attachments` as `file://` URLs).
fn answer_payload(
    db: &mailcore::Db,
    folder_id: i64,
    uid: u32,
    mode: &str,
    files: compose::ForwardFiles,
) -> Result<String, String> {
    let draft = compose::answer_draft_json(db, folder_id, uid, mode).map_err(|e| e.to_string())?;
    let mut draft: serde_json::Value = serde_json::from_str(&draft).map_err(|e| e.to_string())?;
    draft["attachments"] = files
        .files
        .iter()
        .map(|f| {
            serde_json::json!({
                "path": file_url(std::path::Path::new(&f.path)),
                "name": f.name,
            })
        })
        .collect();
    draft["files_notice"] = files.notice.into();
    Ok(draft.to_string())
}
