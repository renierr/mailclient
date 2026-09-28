//! Sending in two halves: a local, instant `prepare_send` and a networked
//! `deliver` that the adapter runs on its net thread.
//!
//! The split is what lets the composer close at once: validation, MIME
//! assembly and the outbox row are pure local work, so a mistake comes back
//! with the composer still open and the text intact. Only the SMTP submit
//! and what follows it wait on the network.
//!
//! An interactive send is explicit consent, so no allow-list applies here —
//! unlike the test harness, which refuses anything outside
//! `MAILCLIENT_TEST_SEND_ALLOWLIST` (`AGENT.md` §6).

use crate::auth;
use crate::models::{Account, FolderRole};
use crate::store::{contacts, folders, messages, queue, settings};
use crate::sync::imap::QUICK_SYNC_WINDOW;
use crate::sync::pool::{checkout_session, job_account, resolve_account};
use crate::sync::sender::{valid_mailboxes, SendFormat, SendPolicy, SmtpSender};
use crate::Db;

use super::drafts::drafts_folder;
use super::ComposeForm;

/// A validated message sitting in the outbox, claimed by its creator and
/// waiting for [`deliver`].
///
/// The row is born claimed (`sending`), so nothing else will submit it. If
/// the adapter cannot start the delivery job after all, it must call
/// [`PreparedSend::discard`] — otherwise crash recovery would later deliver a
/// message the user was told did not go out.
#[derive(Debug)]
pub struct PreparedSend {
    pub queue_id: i64,
    pub account_id: i64,
    raw: Vec<u8>,
    to: Vec<String>,
    cc: Vec<String>,
    bcc: Vec<String>,
    draft_uid: i32,
}

impl PreparedSend {
    /// Drop the built MIME so this row can never be submitted.
    pub fn discard(&self, db: &Db) {
        let _ = queue::discard_mime(db, self.queue_id);
    }
}

/// What happened after SMTP accepted the message. The mail is sent whatever
/// `notes` say; they are the "sent, but …" follow-up failures (Sent copy,
/// source draft, folder refresh).
#[derive(Debug, Default)]
pub struct SendOutcome {
    pub account_id: i64,
    pub notes: Vec<String>,
}

/// Validate the form, build the MIME with the user's send settings and put
/// it in the outbox. No network, no secrets.
pub fn prepare_send(db: &Db, account_id: i64, form: ComposeForm) -> Result<PreparedSend, String> {
    form.require_recipient()?;
    let acc = resolve_account(db, account_id)?;
    let format = SendFormat::parse(&settings::get_send_format(db));
    let include_plain = settings::get_bool(db, settings::COMPOSE_INCLUDE_PLAIN).unwrap_or(true);
    let request_mdn = settings::get_bool(db, settings::REQUEST_MDN).unwrap_or(false);
    let policy = SendPolicy::Unrestricted;
    let (queue_id, raw) = SmtpSender::new(&acc)
        .enqueue_send(
            db,
            acc.id,
            &form.as_request(&acc, format, include_plain, request_mdn, &policy),
        )
        .map_err(|e| e.to_string())?;
    let ComposeForm {
        to,
        cc,
        bcc,
        draft_uid,
        ..
    } = form;
    Ok(PreparedSend {
        queue_id,
        account_id: acc.id,
        raw,
        to,
        cc,
        bcc,
        draft_uid,
    })
}

/// Submit a prepared message, then file it: Sent copy, removal of the draft
/// it came from, and a refresh of Sent plus the folder the user is viewing
/// (`viewed_folder_id`, `-1` for none — a mail sent to self only shows up
/// there through delivery).
///
/// `on_accepted` fires the moment SMTP takes the message, so the adapter can
/// release the composer instead of holding it open through the filing.
///
/// Any failure before or during the submit drops the MIME: an automatic
/// retry could deliver the message twice, and the user who sees the error
/// owns the retry. Nothing after acceptance fails the call — those become
/// [`SendOutcome::notes`], since a failed job would skip the feed refresh and
/// look like nothing happened.
pub async fn deliver(
    db: &Db,
    sent: PreparedSend,
    viewed_folder_id: i64,
    on_accepted: impl FnOnce(),
) -> Result<SendOutcome, String> {
    let prepared = job_account(db, sent.account_id).and_then(|acc| {
        auth::load_account_secrets(&acc.auth_vault_key)
            .map(|s| (acc, s))
            .map_err(|e| format!("no password in keyring: {e}"))
    });
    let (acc, secrets) = match prepared {
        Ok(v) => v,
        Err(e) => {
            sent.discard(db);
            return Err(format!("send failed: {e}"));
        }
    };
    if let Err(e) = SmtpSender::new(&acc).submit_claimed(db, sent.queue_id, &secrets.smtp_password)
    {
        sent.discard(db);
        return Err(format!("send failed: {e}"));
    }
    if settings::get_bool(db, settings::COLLECT_SENT_CONTACTS).unwrap_or(true) {
        collect_recipients(db, &sent.to, &sent.cc, &sent.bcc);
    }
    on_accepted();

    let mut notes = Vec::new();
    // Over the pooled session: connecting again would pay another TCP + TLS +
    // LOGIN while the user waits on mail that is already gone.
    if let Err(e) = save_sent_copy(db, &acc, &sent.raw).await {
        log::warn!("send: sent copy failed: {e}");
        notes.push(format!("sent, but the Sent copy failed: {e}"));
    }
    if sent.draft_uid >= 0 {
        if let Err(e) = remove_source_draft(db, &acc, sent.draft_uid as u32).await {
            notes.push(format!("sent, but {e}"));
        }
    }
    refresh_after_send(db, &acc, viewed_folder_id, &mut notes).await;
    Ok(SendOutcome {
        account_id: acc.id,
        notes,
    })
}

fn collect_recipients(db: &Db, to: &[String], cc: &[String], bcc: &[String]) {
    let mut all = valid_mailboxes(to);
    all.extend(valid_mailboxes(cc));
    all.extend(valid_mailboxes(bcc));
    for mb in all {
        if let Err(e) = contacts::seen(db, mb.email.as_ref(), mb.name.as_deref()) {
            log::warn!("contacts: could not collect recipient: {e}");
        }
    }
}

async fn save_sent_copy(db: &Db, acc: &Account, raw: &[u8]) -> Result<(), String> {
    let mut imap = checkout_session(acc).await?;
    SmtpSender::save_sent_copy_via(db, acc.id, &mut imap, raw)
        .await
        .map_err(|e| e.to_string())?;
    imap.checkin();
    Ok(())
}

async fn remove_source_draft(db: &Db, acc: &Account, uid: u32) -> Result<(), String> {
    let drafts = drafts_folder(db, acc.id)
        .ok_or_else(|| "no Drafts folder is available to remove the source draft".to_string())?;
    let source = messages::get_by_uid(db, drafts.id, uid)
        .map_err(|_| "the source draft no longer exists".to_string())?;
    if !source.is_draft {
        return Err("the source message is not a draft".to_string());
    }
    let mut imap = checkout_session(acc).await?;
    imap.delete_message(db, source.id)
        .await
        .map_err(|e| format!("could not remove the source draft: {e}"))?;
    imap.checkin();
    Ok(())
}

async fn refresh_after_send(db: &Db, acc: &Account, viewed: i64, notes: &mut Vec<String>) {
    let mut targets: Vec<(i64, &'static str)> = Vec::new();
    if let Some(sent) = folders::list_by_account(db, acc.id)
        .unwrap_or_default()
        .into_iter()
        .find(|f| f.role == FolderRole::Sent)
    {
        targets.push((sent.id, "Sent"));
    }
    if viewed >= 0 && !targets.iter().any(|(id, _)| *id == viewed) {
        if let Ok(f) = folders::get(db, viewed) {
            if f.account_id == acc.id {
                targets.push((viewed, "current"));
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAccount;
    use crate::store::accounts;

    fn account(db: &Db, from_name: &str) -> i64 {
        accounts::create(
            db,
            &NewAccount {
                name: "t".to_string(),
                email_address: "me@example.com".to_string(),
                from_name: from_name.to_string(),
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
        .unwrap()
    }

    #[test]
    fn prepare_queues_a_claimed_row_with_the_account_sender_name() {
        let db = Db::open_in_memory().unwrap();
        let id = account(&db, "Account Name");
        let form = ComposeForm::parse(
            r#"{"to":"you@example.com","subject":"queued","body":"hello","draft_uid":5}"#,
        )
        .unwrap();
        let sent = prepare_send(&db, id, form).unwrap();
        assert_eq!(sent.account_id, id);
        assert_eq!(sent.draft_uid, 5);
        let text = String::from_utf8(sent.raw.clone()).unwrap();
        assert!(text.contains("Subject: queued"));
        assert!(text.contains("Account Name"));
        let row = queue::get(&db, sent.queue_id).unwrap();
        assert!(row.raw_mime.as_deref().is_some_and(|b| !b.is_empty()));

        sent.discard(&db);
        let row = queue::get(&db, sent.queue_id).unwrap();
        assert!(row.raw_mime.as_deref().is_none_or(|b| b.is_empty()));
    }

    #[test]
    fn prepare_refuses_before_any_row_exists() {
        let db = Db::open_in_memory().unwrap();
        let id = account(&db, "");
        let empty = ComposeForm::parse(r#"{"subject":"x"}"#).unwrap();
        assert!(prepare_send(&db, id, empty).is_err());
        let bad_cc = ComposeForm::parse(r#"{"to":"you@example.com","cc":"bob@"}"#).unwrap();
        assert!(prepare_send(&db, id, bad_cc).is_err());
        assert!(queue::list_submittable(&db, id).unwrap().is_empty());
    }
}
