//! Composer: send, schedule, drafts and the editor page (step 0d).

use super::*;

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::EnvUnowned;

/// `MailNative.composePrefill(request)`: the composer's starting point for
/// mail begun outside the app — a `mailto:` link or a share — from the
/// `PrefillRequest` JSON (`{mailto?, to?, cc?, bcc?, subject?, text?}`), as
/// `{to, cc, bcc, subject, body_html}`. Local read.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_composePrefill<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    request: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let draft = mailcore::compose::prefill_draft_json(
                crate::db::shared_db()?,
                &string(env, &request)?,
            )?;
            Ok(env.new_string(draft)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// Step 0d composer/send (`android/PLAN.md`): thin wraps of
/// `mailffi::api::composer`. Sends and draft saves queue onto
/// `mailclient-net` (progress/finished events on the 0b listener);
/// validation runs inline, so a bad form throws with the composer open.
///
/// `MailNative.sendMail(accountId, folderId, form)`: validate + MIME + queue.
/// The composer's JSON form is `{to, cc?, bcc?, from?, from_name?,
/// reply_to?, subject, body, body_html?, attachments?, draft_uid?}`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_sendMail<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    folder_id: i64,
    form: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::composer::send_mail(account_id, folder_id, string(env, &form)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.scheduleMail(accountId, form, sendAt)`: validate + MIME +
/// hold as a `scheduled` row. Returns the normalized UTC instant the row
/// waits for, so the app can arm its exact alarm. Throws with the composer
/// open on a bad form or a past time.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_scheduleMail<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    form: JString<'caller>,
    send_at: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let at = crate::api::composer::schedule_mail(
                account_id,
                string(env, &form)?,
                string(env, &send_at)?,
            )?;
            Ok(env.new_string(at)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.saveDraft(accountId, form)`: append the current text to
/// Drafts (created server-side when missing), replacing the opened version.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_saveDraft<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    form: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            crate::api::composer::save_draft(account_id, string(env, &form)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.deleteDraft(accountId, uid)`: destroy a server draft
/// (`\Deleted` + expunge) — what Discard means for a draft from Drafts.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_deleteDraft<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    uid: i32,
) {
    unowned
        .with_env(|_env| -> Result<()> {
            crate::api::composer::delete_draft(account_id, uid_arg(uid)?)?;
            Ok(())
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.answerDraft(folderId, uid, mode)`: reply/reply-all/forward
/// draft JSON — `mode` is `reply`, `reply_all` or `forward`. Local read.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_answerDraft<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
    mode: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::composer::answer_draft(
                folder_id,
                uid_arg(uid)?,
                string(env, &mode)?,
            )?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.forwardFetch(folderId, uid, dir)`: the whole "Forward" job —
/// download the original's missing files, then build the forward seed —
/// like Qt's `forward_fetch`. Queued on the net thread; the
/// `{draft, files}` seed arrives as the finish event's status, so the shell
/// only opens the result. JNI only: the seed is shaped for the native
/// composer (no FRB surface, no codegen).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_forwardFetch<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
    dir: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            let uid = uid_arg(uid)?;
            let dir = string(env, &dir)?;
            crate::net::spawn(
                "Forward",
                format!("forward:{folder_id}:{uid}"),
                move |db, progress| async move {
                    if mailcore::compose::forward_missing(db, folder_id, uid).unwrap_or(0) > 0 {
                        progress.report("Downloading…");
                    }
                    let files = mailcore::compose::forward_files(
                        db,
                        folder_id,
                        uid,
                        std::path::Path::new(&dir),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    let draft: serde_json::Value = serde_json::from_str(
                        &mailcore::compose::answer_draft_json(db, folder_id, uid, "forward")
                            .map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| format!("json: {e}"))?;
                    let seed = serde_json::json!({"draft": draft, "files": files});
                    Ok((seed.to_string(), None))
                },
            )
            .map_err(BridgeError::from)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.resendFetch(folderId, uid, dir)`: `forwardFetch` for "Edit &
/// resend" of the bounce `(folderId, uid)` — the sent original's files plus
/// the resend draft. Same job shape, same finish-event seed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_resendFetch<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    folder_id: i64,
    uid: i32,
    dir: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            let uid = uid_arg(uid)?;
            let dir = string(env, &dir)?;
            crate::net::spawn(
                "Resend",
                format!("resend:{folder_id}:{uid}"),
                move |db, progress| async move {
                    if mailcore::compose::resend_missing(db, folder_id, uid).unwrap_or(0) > 0 {
                        progress.report("Downloading…");
                    }
                    let files = mailcore::compose::resend_files(
                        db,
                        folder_id,
                        uid,
                        std::path::Path::new(&dir),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                    let draft: serde_json::Value = serde_json::from_str(
                        &mailcore::compose::answer_draft_json(db, folder_id, uid, "resend")
                            .map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| format!("json: {e}"))?;
                    let seed = serde_json::json!({"draft": draft, "files": files});
                    Ok((seed.to_string(), None))
                },
            )
            .map_err(BridgeError::from)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.draftFetch(accountId, uid, dir)`: the whole "Open draft" job —
/// download the draft's missing bytes, then build its seed — like Qt's
/// `draft_form`. The `{form, staged}` seed arrives as the finish event's
/// status; `staged` is null when the files could not be staged, and the
/// composer then names them from the form instead of dropping them.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_draftFetch<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    account_id: i64,
    uid: i32,
    dir: JString<'caller>,
) {
    unowned
        .with_env(|env| -> Result<()> {
            let uid = uid_arg(uid)?;
            let dir = string(env, &dir)?;
            crate::net::spawn(
                "Open draft",
                format!("open-draft:{account_id}:{uid}"),
                move |db, progress| async move {
                    let drafts = mailcore::compose::drafts_folder(db, account_id)
                        .ok_or_else(|| "this account has no Drafts folder".to_string())?;
                    let m = mailcore::compose::open_draft(db, drafts.id, uid)
                        .map_err(|e| e.to_string())?;
                    if mailcore::sync::attachments::missing_count(db, m.id, true).unwrap_or(0) > 0 {
                        progress.report("Downloading…");
                    }
                    mailcore::sync::attachments::ensure_cached(db, m.id, true)
                        .await
                        .map_err(|e| e.to_string())?;
                    let staged =
                        mailcore::compose::stage_draft_files(db, m.id, std::path::Path::new(&dir))
                            .ok()
                            .and_then(|files| serde_json::to_value(&files).ok())
                            .unwrap_or(serde_json::Value::Null);
                    let form: serde_json::Value = serde_json::from_str(
                        &crate::api::composer::draft_form_in(db, account_id, uid)
                            .map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| format!("json: {e}"))?;
                    let seed = serde_json::json!({"form": form, "staged": staged});
                    Ok((seed.to_string(), None))
                },
            )
            .map_err(BridgeError::from)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.blankDraft()`: new-mail draft (just the signature), same
/// shape as `answerDraft`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_blankDraft<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::composer::blank_draft()?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.receiptToggles()`: `{read, delivery}`, which receipt
/// toggles the composer shows (`compose::Receipts::offered`).
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_receiptToggles<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let db = crate::db::shared_db()?;
            Ok(env.new_string(mailcore::compose::Receipts::offered_json(db))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.imageDataUrl(path)`: an image file as a `data:` URL for
/// inline display. Throws for non-images and oversize files.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_imageDataUrl<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    path: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::composer::image_data_url(string(env, &path)?)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.senderParts(address)`: the From field split —
/// `{"local","domain"}`, domain locked with its `@`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_senderParts<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    address: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let p = crate::api::composer::sender_parts(string(env, &address)?);
            let json = serde_json::json!({
                "local": p.local,
                "domain": p.domain,
            });
            Ok(env.new_string(serde_json::to_string(&json)?)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.effectiveFrom(local, accountEmail)`: the address a From field
/// sends as.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_effectiveFrom<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    local: JString<'caller>,
    account_email: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            Ok(env.new_string(crate::api::composer::effective_from(
                string(env, &local)?,
                string(env, &account_email)?,
            ))?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.editorDocument(paper, ink, muted, accent, rule, fontPx,
/// placeholder, bodyHtml)`: the WYSIWYG composer page
/// (`mailcore::compose::editor::document`); colours are `0xRRGGBB`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_editorDocument<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    paper: i32,
    ink: i32,
    muted: i32,
    accent: i32,
    rule: i32,
    font_px: i32,
    placeholder: JString<'caller>,
    body_html: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let rgb = |v: i32| (v as u32) & 0xFF_FFFF;
            let style = mailcore::compose::editor::EditorStyle {
                paper: rgb(paper),
                ink: rgb(ink),
                muted: rgb(muted),
                accent: rgb(accent),
                rule: rgb(rule),
                font_px: u32::try_from(font_px.max(0)).unwrap_or(16),
            };
            let doc = mailcore::compose::editor::document(
                &style,
                &string(env, &placeholder)?,
                &string(env, &body_html)?,
            );
            Ok(env.new_string(doc)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}

/// `MailNative.composeFormatNote(sendFormat, html)`: "Sends as plain text",
/// "Sends formatted (HTML)", … for the editor's current body.
#[unsafe(no_mangle)]
pub extern "system" fn Java_de_renier_mailclient_MailNative_composeFormatNote<'caller>(
    mut unowned: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    send_format: JString<'caller>,
    html: JString<'caller>,
) -> JString<'caller> {
    unowned
        .with_env(|env| -> Result<JString<'caller>> {
            let note = mailcore::compose::editor::send_format_note(
                &string(env, &send_format)?,
                &string(env, &html)?,
            );
            Ok(env.new_string(note)?)
        })
        .resolve::<ThrowRuntimeExAndDefault>()
}
