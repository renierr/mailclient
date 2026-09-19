use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::auth;
use mailcore::store::{contacts, folders, messages, queue, settings};
use mailcore::sync::imap::{FULL_SYNC_WINDOW, QUICK_SYNC_WINDOW};
use mailcore::sync::sender::{format_draft, SendFormat, SendPolicy, SendRequest, SmtpSender};

use crate::bridge::messages::{draft_attachment_path, ensure_attachment_data, safe_filename};
use crate::bridge::qobject;
use crate::bridge::qstring;
use crate::bridge::session::{checkout_session, current_account, evict_imap_session};
use crate::bridge::worker::{spawn_job, JobRefresh};

impl qobject::Bridge {
    pub fn send_mail(self: Pin<&mut Self>, form: &QString) -> QString {
        let v: serde_json::Value = match serde_json::from_str(&form.to_string()) {
            Ok(v) => v,
            Err(_) => return qstring("invalid message form"),
        };
        let str_field = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let to_raw = str_field("to");
        let from_raw = str_field("from");
        // Optional Reply-To for our mail ("replies go here instead of From").
        let reply_to_raw = str_field("reply_to");
        // Display name for `From:` — composer field first, else the account
        // default (empty = address only).
        let from_name_raw = str_field("from_name");
        let from_name_composed: Option<String> = if from_name_raw.is_empty() {
            None
        } else {
            Some(from_name_raw)
        };
        let subject = str_field("subject");
        let body = str_field("body");
        // Optional explicit HTML override (new Composer sends both; old
        // payloads only have `body` holding rich HTML source — handled in
        // `resolve_bodies` either way).
        let body_html_raw = str_field("body_html");
        let body_html = if body_html_raw.trim().is_empty() {
            None
        } else {
            Some(body_html_raw)
        };
        let to: Vec<String> = to_raw
            .split([',', ';'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let cc: Vec<String> = str_field("cc")
            .split([',', ';'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let bcc: Vec<String> = str_field("bcc")
            .split([',', ';'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        // To may hold placeholder text or stay blank (BCC-only send) — only
        // all-three-empty blocks here; unparseable To entries are filtered in
        // `send_raw`, which errors when no real recipient remains.
        if to.is_empty() && cc.is_empty() && bcc.is_empty() {
            return qstring("add at least one recipient (To, Cc or Bcc)");
        }
        // Composer FileDialog paths (`attachments: [...]`, plain paths or
        // `file://` URLs). A legacy comma-separated string is also accepted.
        let attachments: Vec<String> = match v.get("attachments") {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            Some(serde_json::Value::String(s)) => s
                .split(['\n', ';'])
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect(),
            _ => Vec::new(),
        };
        let draft_uid = v.get("draft_uid").and_then(|x| x.as_i64()).unwrap_or(-1) as i32;
        let wanted = *self.current_account_id();
        let folder_id = *self.current_folder_id();
        // Validate + build + enqueue synchronously: pure local work (SQLite +
        // file reads), so mistakes report instantly with the composer still
        // open, and the close below never waits on the network. The password
        // fields stay empty — no secret is needed before the SMTP submit.
        let db = match crate::bridge::open_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let acc = match current_account(&db, wanted) {
            Ok(a) => a,
            Err(e) => return qstring(&e),
        };
        let sender = SmtpSender::new(&acc);
        let format = SendFormat::parse(&settings::get_send_format(&db));
        let include_plain =
            settings::get_bool(&db, settings::COMPOSE_INCLUDE_PLAIN).unwrap_or(true);
        let request_mdn = settings::get_bool(&db, settings::REQUEST_MDN).unwrap_or(false);
        let from_name = from_name_composed.or_else(|| {
            let n = acc.from_name.trim().to_string();
            if n.is_empty() {
                None
            } else {
                Some(n)
            }
        });
        let from = if from_raw.is_empty() {
            None
        } else {
            Some(from_raw.as_str())
        };
        let reply_to = (!reply_to_raw.is_empty()).then_some(reply_to_raw.as_str());
        let req = SendRequest {
            to: &to,
            cc: &cc,
            bcc: &bcc,
            from,
            from_name: from_name.as_deref(),
            reply_to,
            subject: &subject,
            body_text: &body,
            body_html: body_html.as_deref(),
            attachments: &attachments,
            format,
            include_plain,
            policy: &SendPolicy::Unrestricted,
            password: "",
            imap_password: None,
            request_mdn,
        };
        let (queue_id, raw) = match sender.enqueue_send(&db, acc.id, &req) {
            Ok(v) => v,
            Err(e) => return qstring(&e.to_string()),
        };
        spawn_job(self, "Send", move |db, progress| async move {
            let acc = current_account(&db, wanted)?;
            let secrets = auth::load_account_secrets(&acc.auth_vault_key)
                .map_err(|e| format!("no password in keyring: {e}"))?;
            let sender = SmtpSender::new(&acc);
            if let Err(e) = sender.submit_queued(&db, queue_id, &secrets.smtp_password) {
                // Same no-duplicate rule as an interactive failure: the user
                // sees this error and owns the retry.
                let _ = queue::discard_mime(&db, queue_id);
                return Err(format!("send failed: {e}"));
            }
            if settings::get_bool(&db, settings::COLLECT_SENT_CONTACTS).unwrap_or(true) {
                let mut all_rcpts = mailcore::sync::sender::valid_mailboxes(&to);
                all_rcpts.extend(mailcore::sync::sender::valid_mailboxes(&cc));
                all_rcpts.extend(mailcore::sync::sender::valid_mailboxes(&bcc));
                for mb in all_rcpts {
                    let addr = mb.email.to_string();
                    let name = mb.name.as_deref();
                    if let Err(e) = contacts::seen(&db, &addr, name) {
                        log::warn!("contacts: could not collect recipient: {e}");
                    }
                }
            }
            // Everything below runs while the user is already free. The mail
            // IS sent, so failures here must not fail the job (which would
            // skip the feed refresh and look like nothing happened) — they
            // become "sent, but …" notes, which close the composer anyway.
            let mut notes: Vec<String> = Vec::new();
            // SMTP accepted it: release the composer now rather than holding
            // it open through the Sent copy, the draft removal and the
            // resync (the Sent copy pays a second TLS + LOGIN of its own).
            progress.report("");
            if let Err(e) = sender
                .save_sent_copy(&db, acc.id, Some(&secrets.imap_password), &raw)
                .await
            {
                log::warn!("send: sent copy failed: {e}");
                notes.push(format!("sent, but the Sent copy failed: {e}"));
            }
            if draft_uid >= 0 {
                let draft_folder = folders::list_by_account(&db, acc.id)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .find(|f| f.role == mailcore::models::FolderRole::Drafts);
                match draft_folder {
                    None => notes.push(
                        "sent, but no Drafts folder is available to remove the source draft"
                            .to_string(),
                    ),
                    Some(draft_folder) => {
                        match messages::get_by_uid(&db, draft_folder.id, draft_uid as u32) {
                            Ok(source) if source.is_draft => {
                                let del_res = async {
                                    let mut imap = checkout_session(&acc).await?;
                                    imap.delete_message(&db, source.id)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                    imap.checkin();
                                    Ok::<_, String>(())
                                }
                                .await;
                                if let Err(e) = del_res {
                                    notes.push(format!(
                                        "sent, but could not remove source draft: {e}"
                                    ));
                                }
                            }
                            Ok(_) => notes
                                .push("sent, but the source message is not a draft".to_string()),
                            Err(_) => notes
                                .push("sent, but the source draft no longer exists".to_string()),
                        }
                    }
                }
            }
            sent_resync(&db, &acc, folder_id, notes).await
        })
    }

    pub fn save_draft(self: Pin<&mut Self>, form: &QString) -> QString {
        let v: serde_json::Value = match serde_json::from_str(&form.to_string()) {
            Ok(v) => v,
            Err(_) => return qstring("invalid draft form"),
        };
        let str_field = |k: &str| {
            v.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let split_addresses = |key: &str| {
            str_field(key)
                .split([',', ';'])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        };
        let from_raw = str_field("from");
        let from_name_raw = str_field("from_name");
        let reply_to_raw = str_field("reply_to");
        let body = str_field("body");
        let body_html_raw = str_field("body_html");
        let to = split_addresses("to");
        let cc = split_addresses("cc");
        let bcc = split_addresses("bcc");
        let subject = str_field("subject");
        let attachments = match v.get("attachments") {
            Some(serde_json::Value::Array(items)) => items
                .iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            _ => Vec::new(),
        };
        let source_uid = v.get("draft_uid").and_then(|x| x.as_i64()).unwrap_or(-1) as i32;
        let wanted = *self.current_account_id();
        let current_folder_id = *self.current_folder_id();
        spawn_job(self, "Save draft", move |db, _progress| async move {
            let acc = current_account(&db, wanted)?;
            let drafts = match folders::list_by_account(&db, acc.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|f| f.role == mailcore::models::FolderRole::Drafts)
            {
                Some(d) => d,
                None => {
                    // No Drafts folder on this account yet: create one
                    // server-side so saving always works (mirrors the
                    // Archive auto-create on the archive path).
                    let delimiter = folders::list_by_account(&db, acc.id)
                        .unwrap_or_default()
                        .first()
                        .map(|f| f.delimiter.clone())
                        .unwrap_or_else(|| "/".to_string());
                    let mut imap = checkout_session(&acc).await?;
                    let folder = imap
                        .create_folder_path(&db, acc.id, "Drafts", &delimiter)
                        .await
                        .map_err(|e| e.to_string())?;
                    imap.checkin();
                    folder
                }
            };
            if source_uid >= 0 {
                let source = messages::get_by_uid(&db, drafts.id, source_uid as u32)
                    .map_err(|_| "source draft no longer exists".to_string())?;
                if !source.is_draft {
                    return Err("source message is not a draft".to_string());
                }
            }
            let from = (!from_raw.is_empty()).then_some(from_raw.as_str());
            let from_name = (!from_name_raw.is_empty()).then_some(from_name_raw.as_str());
            let reply_to = (!reply_to_raw.is_empty()).then_some(reply_to_raw.as_str());
            let body_html = (!body_html_raw.is_empty()).then_some(body_html_raw.as_str());
            let req = SendRequest {
                to: &to,
                cc: &cc,
                bcc: &bcc,
                from,
                from_name,
                reply_to,
                subject: &subject,
                body_text: &body,
                body_html,
                attachments: &attachments,
                format: SendFormat::Multipart,
                include_plain: true,
                policy: &SendPolicy::Unrestricted,
                password: "",
                imap_password: None,
                request_mdn: false,
            };
            let raw = format_draft(&acc, &req).map_err(|e| e.to_string())?;
            let mut imap = checkout_session(&acc).await?;
            imap.append_draft(&drafts.path, &raw)
                .await
                .map_err(|e| e.to_string())?;
            let remove_error = if source_uid >= 0 {
                let source = messages::get_by_uid(&db, drafts.id, source_uid as u32)
                    .map_err(|_| "source draft no longer exists".to_string())?;
                imap.delete_message(&db, source.id)
                    .await
                    .err()
                    .map(|e| e.to_string())
            } else {
                None
            };
            imap.sync_folder_window(&db, drafts.id, Some(FULL_SYNC_WINDOW))
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            if let Some(e) = remove_error {
                evict_imap_session(acc.id);
                return Err(format!(
                    "draft saved, but could not remove source draft: {e}"
                ));
            }
            Ok((
                String::new(),
                Some(JobRefresh::feeds(acc.id, current_folder_id)),
            ))
        })
    }

    pub fn draft_form(self: Pin<&mut Self>, uid: i32) -> QString {
        let folder_id = *self.current_folder_id();
        spawn_job(self, "Open draft", move |db, _progress| async move {
            let folder = folders::get(&db, folder_id).map_err(|_| "unknown folder".to_string())?;
            let message = messages::get_by_uid(&db, folder_id, uid as u32)
                .map_err(|_| "draft is no longer available".to_string())?;
            if folder.role != mailcore::models::FolderRole::Drafts || !message.is_draft {
                return Err("message is not a draft".to_string());
            }
            ensure_attachment_data(&db, message.id, true).await?;
            let attachments = messages::list_attachments(&db, message.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|a| {
                    let path = draft_attachment_path(&db, a.id)?;
                    Ok(serde_json::json!({
                        "path": path,
                        "name": safe_filename(a.filename.as_deref(), a.id),
                    }))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok((
                serde_json::json!({
                    "draft_uid": message.uid,
                    "from": message.from_addr.unwrap_or_default(),
                    "reply_to": message.reply_to.unwrap_or_default(),
                    "to": message.to_addrs.join(", "),
                    "cc": message.cc_addrs.join(", "),
                    "bcc": message.bcc_addrs.join(", "),
                    "subject": message.subject.unwrap_or_default(),
                    "body": message.body_html.or(message.body_text).unwrap_or_default(),
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
            let acc = current_account(&db, wanted)?;
            let drafts = folders::list_by_account(&db, acc.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|f| f.role == mailcore::models::FolderRole::Drafts)
                .ok_or_else(|| "draft is no longer available".to_string())?;
            let msg = messages::get_by_uid(&db, drafts.id, uid as u32)
                .map_err(|_| "draft is no longer available".to_string())?;
            if !msg.is_draft {
                return Err("message is not a draft".to_string());
            }
            // Drafts are destroyed outright, never filed to Trash:
            // discarding an unsent draft means it is gone.
            let mut imap = checkout_session(&acc).await?;
            imap.delete_message(&db, msg.id)
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok((
                "Draft deleted".to_string(),
                Some(JobRefresh::feeds(acc.id, current)),
            ))
        })
    }
}

/// Resync after a send and always refresh the feeds: the Sent folder (the
/// copy just landed there) plus the viewed folder (a mail sent to self only
/// arrives via delivery, so without this the list sits stale until the next
/// manual refresh). The mail IS sent at this point, so even a failed resync
/// reports ("sent, but …") instead of failing the job — a failed job skips
/// the feed refresh and looks exactly like "the folder did not update",
/// with no reason shown.
async fn sent_resync(
    db: &mailcore::Db,
    acc: &mailcore::models::Account,
    folder_id: i64,
    mut notes: Vec<String>,
) -> Result<(String, Option<JobRefresh>), String> {
    let mut targets: Vec<(i64, &'static str)> = Vec::new();
    if let Some(sent) = folders::list_by_account(db, acc.id)
        .unwrap_or_default()
        .into_iter()
        .find(|f| f.role == mailcore::models::FolderRole::Sent)
        .map(|f| f.id)
    {
        targets.push((sent, "Sent"));
    }
    if folder_id >= 0 && !targets.iter().any(|(id, _)| *id == folder_id) {
        match folders::get(db, folder_id) {
            Ok(f) if f.account_id == acc.id => targets.push((folder_id, "current")),
            _ => {}
        }
    }
    for (id, label) in targets {
        let res = async {
            let mut imap = checkout_session(acc).await?;
            imap.sync_folder_window(db, id, Some(QUICK_SYNC_WINDOW))
                .await
                .map_err(|e| e.to_string())?;
            imap.checkin();
            Ok::<_, String>(())
        }
        .await;
        if let Err(e) = res {
            log::warn!("send: {label} resync failed: {e}");
            notes.push(format!("sent, but the {label} folder did not refresh: {e}"));
        }
    }
    Ok((notes.join(" "), Some(JobRefresh::feeds(acc.id, folder_id))))
}
