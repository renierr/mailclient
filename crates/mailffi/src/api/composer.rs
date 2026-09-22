//! Sending and drafts.

use mailcore::auth;
use mailcore::models::FolderRole;
use mailcore::store::{contacts, folders, messages, queue, settings};
use mailcore::sync::sender::{format_draft, SendFormat, SendPolicy, SmtpSender};

use crate::api::form::ComposeForm;
use crate::db::shared_db;
use crate::net::{spawn, JobRefresh};
use crate::session::{checkout_session, resolve_account};

/// Send a message from the composer's JSON form
/// (`{to, cc?, bcc?, from?, from_name?, reply_to?, subject, body, body_html?,
/// attachments?, draft_uid?}`).
///
/// Validation, MIME assembly and queueing happen **inline**, before this
/// returns: that is pure local work (SQLite plus file reads), so a mistake
/// comes back instantly with the composer still open and the text intact.
/// Only the SMTP submit is queued, which is why the composer can close on the
/// `Send` job's *progress* event instead of waiting for the Sent copy and the
/// resync that follow it.
///
/// An interactive send is explicit consent, so no allow-list applies here —
/// unlike the test harness, which refuses anything outside
/// `MAILCLIENT_TEST_SEND_ALLOWLIST` (`AGENT.md` §6).
pub fn send_mail(account_id: i64, folder_id: i64, form: String) -> anyhow::Result<()> {
    let form = ComposeForm::parse(&form)?;
    form.require_recipient()?;

    let db = shared_db()?;
    let acc = resolve_account(db, account_id).map_err(|e| anyhow::anyhow!(e))?;
    let format = SendFormat::parse(&settings::get_send_format(db));
    let include_plain = settings::get_bool(db, settings::COMPOSE_INCLUDE_PLAIN).unwrap_or(true);
    let request_mdn = settings::get_bool(db, settings::REQUEST_MDN).unwrap_or(false);
    let policy = SendPolicy::Unrestricted;
    let (queue_id, raw) = SmtpSender::new(&acc).enqueue_send(
        db,
        acc.id,
        &form.as_request(&acc, format, include_plain, request_mdn, &policy),
    )?;

    let ComposeForm {
        to,
        cc,
        bcc,
        draft_uid,
        ..
    } = form;
    let account_id = acc.id;
    spawn(
        "Send",
        format!("send:{queue_id}"),
        move |db, progress| async move {
            let acc = resolve_account(db, account_id)?;
            let secrets = auth::load_account_secrets(&acc.auth_vault_key)
                .map_err(|e| format!("no password in keyring: {e}"))?;
            if let Err(e) =
                SmtpSender::new(&acc).submit_queued(db, queue_id, &secrets.smtp_password)
            {
                // Drop the built MIME rather than leaving it queued: an automatic
                // retry could deliver the message twice. The user sees the error
                // and owns the retry.
                let _ = queue::discard_mime(db, queue_id);
                return Err(format!("send failed: {e}"));
            }
            if settings::get_bool(db, settings::COLLECT_SENT_CONTACTS).unwrap_or(true) {
                collect_recipients(db, &to, &cc, &bcc);
            }

            // SMTP accepted it, so the message *is* sent. Release the composer
            // here rather than holding it open through the Sent copy, the draft
            // removal and the resync — and for the same reason nothing below may
            // fail the job: that would skip the refresh and look like nothing
            // happened. Failures become "sent, but …" notes instead.
            progress.report("");

            let mut notes: Vec<String> = Vec::new();
            // Over the pooled session: connecting again would pay another TCP +
            // TLS + LOGIN while the user waits on mail that is already gone.
            if let Err(e) = save_sent_copy(db, &acc, &raw).await {
                log::warn!("send: sent copy failed: {e}");
                notes.push(format!("sent, but the Sent copy failed: {e}"));
            }
            if draft_uid >= 0 {
                if let Err(e) = remove_source_draft(db, &acc, draft_uid as u32).await {
                    notes.push(format!("sent, but {e}"));
                }
            }

            let mut status = if notes.is_empty() {
                "Sent".to_string()
            } else {
                notes.join("; ")
            };
            // Sent has a new message in it; the folder the user is looking at may
            // have lost the draft this came from.
            if let Some(sent) = folder_by_role(db, acc.id, FolderRole::Sent) {
                if let Err(e) = resync_folder(db, &acc, sent.id).await {
                    log::warn!("send: Sent resync failed: {e}");
                    status.push_str("; Sent will refresh on the next sync");
                }
            }
            Ok((status, Some(JobRefresh::folder(acc.id, folder_id))))
        },
    )
}

/// Append the composer's current text to Drafts, replacing what it was
/// opened from.
///
/// IMAP has no "edit a message": a replace is an APPEND of the new version
/// plus an expunge of the old, which is why this is a queued job and not
/// something that can run while the user keeps typing. The Drafts folder is
/// created server-side when the account has none, the way archiving creates
/// Archive.
pub fn save_draft(account_id: i64, form: String) -> anyhow::Result<()> {
    let form = ComposeForm::parse(&form)?;
    spawn(
        "Save draft",
        format!("draft:{account_id}"),
        move |db, _progress| async move {
            let acc = resolve_account(db, account_id)?;
            let drafts = match folder_by_role(db, acc.id, FolderRole::Drafts) {
                Some(d) => d,
                None => {
                    let mut imap = checkout_session(&acc).await?;
                    let created = imap
                        .create_folder_path(db, acc.id, "Drafts", &account_delimiter(db, acc.id))
                        .await
                        .map_err(|e| e.to_string())?;
                    imap.checkin();
                    created
                }
            };
            // Check the source still exists before appending, so a stale composer
            // cannot leave two copies behind.
            if form.draft_uid >= 0 {
                let source = messages::get_by_uid(db, drafts.id, form.draft_uid as u32)
                    .map_err(|_| "the source draft no longer exists".to_string())?;
                if !source.is_draft {
                    return Err("the source message is not a draft".to_string());
                }
            }
            // A draft always keeps its rich text, whatever the user's *send*
            // format preference is — that preference applies to sending.
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
            let mut note = String::new();
            if form.draft_uid >= 0 {
                if let Ok(source) = messages::get_by_uid(db, drafts.id, form.draft_uid as u32) {
                    if let Err(e) = imap.delete_message(db, source.id).await {
                        // The new version is already on the server, so leaving
                        // the old one is untidy rather than destructive.
                        note = format!(" (the previous version could not be removed: {e})");
                    }
                }
            }
            imap.checkin();
            resync_folder(db, &acc, drafts.id).await?;
            Ok((
                format!("Draft saved{note}"),
                Some(JobRefresh::folder(acc.id, drafts.id)),
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
    let acc = resolve_account(db, account_id).map_err(|e| anyhow::anyhow!(e))?;
    let drafts = folder_by_role(db, acc.id, FolderRole::Drafts)
        .ok_or_else(|| anyhow::anyhow!("this account has no Drafts folder"))?;
    let m = messages::get_by_uid(db, drafts.id, uid)?;
    if !m.is_draft {
        anyhow::bail!("that message is not a draft");
    }
    // The composer edits addresses as one line per field.
    let line = |addrs: &[String]| addrs.join(", ");
    let attachments: serde_json::Value =
        serde_json::from_str(&mailcore::feed::attachments_json(db, drafts.id, uid)?)
            .unwrap_or_else(|_| serde_json::Value::Array(Vec::new()));
    Ok(serde_json::json!({
        "draft_uid": uid,
        "from": m.from_addr.unwrap_or_default(),
        "to": line(&m.to_addrs),
        "cc": line(&m.cc_addrs),
        "bcc": line(&m.bcc_addrs),
        "reply_to": m.reply_to.unwrap_or_default(),
        "subject": m.subject.unwrap_or_default(),
        "body": m.body_text.unwrap_or_default(),
        "body_html": m.body_html.unwrap_or_default(),
        "attachments": attachments,
    })
    .to_string())
}

/// Destroy a server draft (`\Deleted` + expunge, never filed to Trash) —
/// what Discard means for a draft opened from the Drafts folder.
pub fn delete_draft(account_id: i64, uid: u32) -> anyhow::Result<()> {
    let key = format!("draft-del:{account_id}:{uid}");
    spawn("Delete draft", key, move |db, _progress| async move {
        let acc = resolve_account(db, account_id)?;
        let drafts = folder_by_role(db, acc.id, FolderRole::Drafts)
            .ok_or_else(|| "this account has no Drafts folder".to_string())?;
        let m = messages::get_by_uid(db, drafts.id, uid).map_err(|e| e.to_string())?;
        if !m.is_draft {
            return Err("that message is not a draft".to_string());
        }
        let mut imap = checkout_session(&acc).await?;
        let result = imap.delete_message(db, m.id).await;
        imap.checkin();
        result.map_err(|e| e.to_string())?;
        Ok((
            "Draft discarded".to_string(),
            Some(JobRefresh::folder(acc.id, drafts.id)),
        ))
    })
}

fn folder_by_role(
    db: &mailcore::Db,
    account_id: i64,
    role: FolderRole,
) -> Option<mailcore::models::Folder> {
    folders::list_by_account(db, account_id)
        .unwrap_or_default()
        .into_iter()
        .find(|f| f.role == role)
}

fn account_delimiter(db: &mailcore::Db, account_id: i64) -> String {
    folders::list_by_account(db, account_id)
        .unwrap_or_default()
        .first()
        .map(|f| f.delimiter.clone())
        .unwrap_or_else(|| "/".to_string())
}

fn collect_recipients(db: &mailcore::Db, to: &[String], cc: &[String], bcc: &[String]) {
    let mut all = mailcore::sync::sender::valid_mailboxes(to);
    all.extend(mailcore::sync::sender::valid_mailboxes(cc));
    all.extend(mailcore::sync::sender::valid_mailboxes(bcc));
    for mb in all {
        if let Err(e) = contacts::seen(db, mb.email.as_ref(), mb.name.as_deref()) {
            log::warn!("contacts: could not collect recipient: {e}");
        }
    }
}

async fn save_sent_copy(
    db: &mailcore::Db,
    acc: &mailcore::models::Account,
    raw: &[u8],
) -> Result<(), String> {
    let mut imap = checkout_session(acc).await?;
    let result = SmtpSender::save_sent_copy_via(db, acc.id, &mut imap, raw).await;
    imap.checkin();
    result.map_err(|e| e.to_string())
}

async fn remove_source_draft(
    db: &mailcore::Db,
    acc: &mailcore::models::Account,
    uid: u32,
) -> Result<(), String> {
    let drafts = folder_by_role(db, acc.id, FolderRole::Drafts)
        .ok_or_else(|| "no Drafts folder is available to remove the source draft".to_string())?;
    let source = messages::get_by_uid(db, drafts.id, uid)
        .map_err(|_| "the source draft no longer exists".to_string())?;
    if !source.is_draft {
        return Err("the source message is not a draft".to_string());
    }
    let mut imap = checkout_session(acc).await?;
    let result = imap.delete_message(db, source.id).await;
    imap.checkin();
    result.map_err(|e| format!("could not remove the source draft: {e}"))
}

/// Pull a folder's newest window down again after we changed it server-side.
async fn resync_folder(
    db: &mailcore::Db,
    acc: &mailcore::models::Account,
    folder_id: i64,
) -> Result<(), String> {
    use mailcore::sync::imap::QUICK_SYNC_WINDOW;
    let mut imap = checkout_session(acc).await?;
    let result = imap
        .sync_folder_window(db, folder_id, Some(QUICK_SYNC_WINDOW))
        .await;
    imap.checkin();
    result.map(|_| ()).map_err(|e| e.to_string())
}
