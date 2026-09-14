use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::auth;
use mailcore::store::{folders, messages, settings};
use mailcore::sync::imap::{FULL_SYNC_WINDOW, QUICK_SYNC_WINDOW};
use mailcore::sync::sender::{format_draft, SendFormat, SendPolicy, SendRequest, SmtpSender};
use mailcore::sync::traits::MailSender;

use crate::bridge::messages::{draft_attachment_path, ensure_attachment_data, safe_filename};
use crate::bridge::qobject;
use crate::bridge::session::{current_account, evict_imap_session, with_imap};
use crate::bridge::worker::{spawn_job, JobRefresh};
use crate::bridge::qstring;

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
        spawn_job(self, "Send", move |db| {
            let acc = current_account(db, wanted)?;
            let secrets = auth::load_account_secrets(&acc.auth_vault_key)
                .map_err(|e| format!("no password in keyring: {e}"))?;
            let mut sender = SmtpSender::new(&acc);
            let format = SendFormat::parse(&settings::get_send_format(db));
            let include_plain =
                settings::get_bool(db, settings::COMPOSE_INCLUDE_PLAIN).unwrap_or(true);
            let request_mdn = settings::get_bool(db, settings::REQUEST_MDN).unwrap_or(false);
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
            let req = SendRequest {
                to: &to,
                cc: &cc,
                bcc: &bcc,
                from,
                from_name: from_name.as_deref(),
                subject: &subject,
                body_text: &body,
                body_html: body_html.as_deref(),
                attachments: &attachments,
                format,
                include_plain,
                policy: &SendPolicy::Unrestricted,
                password: &secrets.smtp_password,
                imap_password: Some(&secrets.imap_password),
                request_mdn,
            };
            sender
                .send_raw(db, acc.id, &req)
                .map_err(|e| e.to_string())?;
            if draft_uid >= 0 {
                let draft_folder = folders::list_by_account(db, acc.id)
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .find(|f| f.role == mailcore::models::FolderRole::Drafts)
                    .ok_or_else(|| {
                        "sent, but no Drafts folder is available to remove the source draft"
                            .to_string()
                    })?;
                let source = messages::get_by_uid(db, draft_folder.id, draft_uid as u32)
                    .map_err(|_| "sent, but the source draft no longer exists".to_string())?;
                if !source.is_draft {
                    return Err("sent, but the source message is not a draft".to_string());
                }
                with_imap(&acc, |imap| {
                    imap.delete_message(db, source.id)
                        .map_err(|e| e.to_string())
                })
                .map_err(|e| format!("sent, but could not remove source draft: {e}"))?;
            }
            if let Some(sent) = folders::list_by_account(db, acc.id)
                .unwrap_or_default()
                .into_iter()
                .find(|f| f.role == mailcore::models::FolderRole::Sent)
                .map(|f| f.id)
            {
                let _ = with_imap(&acc, |imap| {
                    imap.sync_folder_window(db, sent, Some(QUICK_SYNC_WINDOW))
                        .map_err(|e| e.to_string())
                });
            }
            Ok((
                String::new(),
                JobRefresh {
                    account_id: acc.id,
                    folder_id,
                    message_limit: None,
                },
            ))
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
        spawn_job(self, "Save draft", move |db| {
            let acc = current_account(db, wanted)?;
            let drafts = folders::list_by_account(db, acc.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|f| f.role == mailcore::models::FolderRole::Drafts)
                .ok_or_else(|| "no server Drafts folder found; sync folders first".to_string())?;
            if source_uid >= 0 {
                let source = messages::get_by_uid(db, drafts.id, source_uid as u32)
                    .map_err(|_| "source draft no longer exists".to_string())?;
                if !source.is_draft {
                    return Err("source message is not a draft".to_string());
                }
            }
            let from = (!from_raw.is_empty()).then_some(from_raw.as_str());
            let from_name = (!from_name_raw.is_empty()).then_some(from_name_raw.as_str());
            let body_html = (!body_html_raw.is_empty()).then_some(body_html_raw.as_str());
            let req = SendRequest {
                to: &to,
                cc: &cc,
                bcc: &bcc,
                from,
                from_name,
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
            let remove_error = with_imap(&acc, |imap| {
                imap.append_draft(&drafts.path, &raw)
                    .map_err(|e| e.to_string())?;
                let remove_error = if source_uid >= 0 {
                    let source = messages::get_by_uid(db, drafts.id, source_uid as u32)
                        .map_err(|_| "source draft no longer exists".to_string())?;
                    imap.delete_message(db, source.id)
                        .err()
                        .map(|e| e.to_string())
                } else {
                    None
                };
                imap.sync_folder_window(db, drafts.id, Some(FULL_SYNC_WINDOW))
                    .map_err(|e| e.to_string())?;
                Ok(remove_error)
            })?;
            if let Some(e) = remove_error {
                evict_imap_session(acc.id);
                return Err(format!(
                    "draft saved, but could not remove source draft: {e}"
                ));
            }
            Ok((
                String::new(),
                JobRefresh {
                    account_id: acc.id,
                    folder_id: current_folder_id,
                    message_limit: None,
                },
            ))
        })
    }

    pub fn draft_form(self: Pin<&mut Self>, uid: i32) -> QString {
        let folder_id = *self.current_folder_id();
        let acc_id = *self.current_account_id();
        spawn_job(self, "Open draft", move |db| {
            let folder = folders::get(db, folder_id).map_err(|_| "unknown folder".to_string())?;
            let message = messages::get_by_uid(db, folder_id, uid as u32)
                .map_err(|_| "draft is no longer available".to_string())?;
            if folder.role != mailcore::models::FolderRole::Drafts || !message.is_draft {
                return Err("message is not a draft".to_string());
            }
            ensure_attachment_data(db, message.id, true)?;
            let attachments = messages::list_attachments(db, message.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|a| {
                    let path = draft_attachment_path(db, a.id)?;
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
                    "to": message.to_addrs.join(", "),
                    "cc": message.cc_addrs.join(", "),
                    "bcc": message.bcc_addrs.join(", "),
                    "subject": message.subject.unwrap_or_default(),
                    "body": message.body_html.or(message.body_text).unwrap_or_default(),
                    "attachments": attachments,
                })
                .to_string(),
                JobRefresh {
                    account_id: acc_id,
                    folder_id,
                    message_limit: None,
                },
            ))
        })
    }
}
