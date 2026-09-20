//! mailjni — In-process JNI shared library (`libmailcore_jni.so`).
//!
//! Exposes `mailcore` directly to the Kotlin JVM desktop (and Android) client
//! in-process with sub-millisecond execution, persistent SQLite handles, and
//! async Tokio runtime for IMAP/SMTP sessions.

use std::path::PathBuf;
use std::sync::Mutex;

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong, jstring};
use jni::JNIEnv;
use once_cell::sync::OnceCell;

use mailcore::store::{accounts, folders, messages, queue};
use mailcore::sync::headless;
use mailcore::{default_db_path, feed, Db};

struct NativeState {
    db: Db,
    rt: tokio::runtime::Runtime,
}

static STATE: OnceCell<Mutex<NativeState>> = OnceCell::new();

fn resolve_secrets(acc: &mailcore::models::Account) -> (String, String) {
    if let Ok(secrets) = mailcore::auth::load_account_secrets(&acc.auth_vault_key) {
        if !secrets.imap_password.is_empty() {
            return (secrets.imap_password, secrets.smtp_password);
        }
    }
    let test_imap = std::env::var("MAILCLIENT_TEST_IMAP_PASS").unwrap_or_default();
    let test_smtp = std::env::var("MAILCLIENT_TEST_SMTP_PASS").unwrap_or_default();
    (test_imap, test_smtp)
}

fn to_jstring<'a>(env: &mut JNIEnv<'a>, s: &str) -> jstring {
    env.new_string(s)
        .expect("cannot allocate Java string")
        .into_raw()
}

fn get_str(env: &mut JNIEnv, js: &JString) -> String {
    if js.as_raw().is_null() {
        return String::new();
    }
    env.get_string(js).map(|s| s.into()).unwrap_or_default()
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeInit(
    mut env: JNIEnv,
    _class: JClass,
    db_path: JString,
) {
    dotenvy::dotenv().ok();
    let override_path = get_str(&mut env, &db_path);
    let path = if !override_path.trim().is_empty() {
        PathBuf::from(override_path.trim())
    } else {
        default_db_path()
    };

    STATE.get_or_init(|| {
        let db = Db::open(&path).unwrap_or_else(|e| {
            panic!("cannot open database at {}: {e}", path.display());
        });
        let rt = tokio::runtime::Runtime::new().expect("cannot create tokio runtime");
        Mutex::new(NativeState { db, rt })
    });
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeAccounts(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let json = feed::accounts_json(&state.db).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    to_jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeFolders(
    mut env: JNIEnv,
    _class: JClass,
    account_id: jlong,
) -> jstring {
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let json = feed::folders_json(&state.db, account_id)
        .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    to_jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeMessages(
    mut env: JNIEnv,
    _class: JClass,
    folder_id: jlong,
    limit: jint,
    offset: jint,
) -> jstring {
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let json = feed::messages_list_json_paged(
        &state.db,
        folder_id,
        (limit.max(1)) as u64,
        (offset.max(0)) as u64,
    )
    .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    to_jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeMessage(
    mut env: JNIEnv,
    _class: JClass,
    folder_id: jlong,
    uid: jlong,
) -> jstring {
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let json = feed::message_json(&state.db, folder_id, uid as u32)
        .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    to_jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeSearch(
    mut env: JNIEnv,
    _class: JClass,
    account_id: jlong,
    query: JString,
    folder: JString,
    limit: jint,
) -> jstring {
    let q = get_str(&mut env, &query);
    let f = get_str(&mut env, &folder);
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let json = feed::search_json(&state.db, account_id, &q, limit.max(1) as u64, &f)
        .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    to_jstring(&mut env, &json)
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeMarkRead(
    _env: JNIEnv,
    _class: JClass,
    folder_id: jlong,
    uid: jlong,
    read: jboolean,
) {
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let _ = messages::set_read_many_by_uids(&state.db, folder_id, &[uid as u32], read != 0);
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeMarkStar(
    _env: JNIEnv,
    _class: JClass,
    folder_id: jlong,
    uid: jlong,
    starred: jboolean,
) {
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let _ = messages::set_star_many_by_uids(&state.db, folder_id, &[uid as u32], starred != 0);
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeStatus(
    mut env: JNIEnv,
    _class: JClass,
    account_id: jlong,
) -> jstring {
    let filter = if account_id >= 0 { Some(account_id) } else { None };
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let mut accs = headless::unread_summary(&state.db);
    if let Some(id) = filter {
        accs.retain(|a| a.account_id == id);
    }
    let unread: u64 = accs.iter().map(|a| a.unread).sum();
    let recent = headless::recent_unread(&state.db, 10, filter);
    let payload = serde_json::json!({
        "ok": true,
        "unread": unread,
        "accounts": accs,
        "recent": recent,
    });
    to_jstring(&mut env, &payload.to_string())
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeSync(
    mut env: JNIEnv,
    _class: JClass,
    account_id: jlong,
) -> jstring {
    let filter = if account_id >= 0 { Some(account_id) } else { None };
    let db_path = default_db_path();
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();

    let _guard = match headless::acquire_sync_lock(&db_path) {
        Ok(g) => g,
        Err(e) => {
            let err_json = serde_json::json!({
                "ok": false,
                "error": format!("sync lock: {e}"),
                "locked": true,
            });
            return to_jstring(&mut env, &err_json.to_string());
        }
    };
    if _guard.is_none() {
        let payload = serde_json::json!({"ok": true, "locked": true});
        return to_jstring(&mut env, &payload.to_string());
    }

    let mut report = headless::sync_all_accounts_blocking(&state.db);
    if let Some(id) = filter {
        report.accounts.retain(|a| a.account_id == id);
        report.total_unread = report.accounts.iter().map(|a| a.unread).sum();
        report.total_fetched = report.accounts.iter().map(|a| a.fetched).sum();
        report.total_expunged = report.accounts.iter().map(|a| a.expunged).sum();
    }
    let payload = serde_json::json!({
        "ok": true,
        "locked": false,
        "unread": report.total_unread,
        "fetched": report.total_fetched,
        "expunged": report.total_expunged,
        "accounts": report.accounts,
        "errors": report.errors,
    });
    to_jstring(&mut env, &payload.to_string())
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeSend(
    mut env: JNIEnv,
    _class: JClass,
    account_id: jlong,
    to: JString,
    cc: JString,
    bcc: JString,
    subject: JString,
    body: JString,
    body_html: JString,
    reply_to: JString,
    attachments: JString,
) -> jstring {
    let to_str = get_str(&mut env, &to);
    let cc_str = get_str(&mut env, &cc);
    let bcc_str = get_str(&mut env, &bcc);
    let subject_str = get_str(&mut env, &subject);
    let body_str = get_str(&mut env, &body);
    let html_str = get_str(&mut env, &body_html);
    let reply_to_str = get_str(&mut env, &reply_to);
    let att_str = get_str(&mut env, &attachments);

    let to_vec: Vec<String> = to_str.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let cc_vec: Vec<String> = cc_str.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let bcc_vec: Vec<String> = bcc_str.split([',', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let att_vec: Vec<String> = att_str.split([';', '\n']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

    if to_vec.is_empty() && cc_vec.is_empty() && bcc_vec.is_empty() {
        let err = serde_json::json!({"ok": false, "error": "add at least one recipient"});
        return to_jstring(&mut env, &err.to_string());
    }

    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let acc = match accounts::get(&state.db, account_id) {
        Ok(a) => a,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("account not found: {e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };

    let (imap_pw, smtp_pw) = resolve_secrets(&acc);
    if smtp_pw.is_empty() {
        let err = serde_json::json!({"ok": false, "error": "no SMTP password in keyring or .env"});
        return to_jstring(&mut env, &err.to_string());
    }

    let sender = mailcore::sync::sender::SmtpSender::new(&acc);
    let from_name = (!acc.from_name.trim().is_empty()).then_some(acc.from_name.as_str());
    let reply_to_opt = (!reply_to_str.trim().is_empty()).then_some(reply_to_str.as_str());
    let html_opt = (!html_str.trim().is_empty()).then_some(html_str.as_str());

    let req = mailcore::sync::sender::SendRequest {
        to: &to_vec,
        cc: &cc_vec,
        bcc: &bcc_vec,
        from: Some(&acc.email_address),
        from_name,
        reply_to: reply_to_opt,
        subject: &subject_str,
        body_text: &body_str,
        body_html: html_opt,
        attachments: &att_vec,
        format: mailcore::sync::sender::SendFormat::Auto,
        include_plain: true,
        policy: &mailcore::sync::sender::SendPolicy::Unrestricted,
        password: "",
        imap_password: None,
        request_mdn: false,
    };

    let (queue_id, raw) = match sender.enqueue_send(&state.db, acc.id, &req) {
        Ok(v) => v,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("enqueue error: {e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };

    if let Err(e) = sender.submit_queued(&state.db, queue_id, &smtp_pw) {
        let _ = queue::discard_mime(&state.db, queue_id);
        let err = serde_json::json!({"ok": false, "error": format!("SMTP submit error: {e}")});
        return to_jstring(&mut env, &err.to_string());
    }

    let imap_pw_ref = if imap_pw.is_empty() { None } else { Some(imap_pw.as_str()) };
    let _ = state.rt.block_on(async {
        sender.save_sent_copy(&state.db, acc.id, imap_pw_ref, &raw).await
    });

    let ok = serde_json::json!({"ok": true, "message": "Mail sent"});
    to_jstring(&mut env, &ok.to_string())
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeDelete(
    mut env: JNIEnv,
    _class: JClass,
    folder_id: jlong,
    uid: jlong,
) -> jstring {
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let msg = match messages::get_by_uid(&state.db, folder_id, uid as u32) {
        Ok(m) => m,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };
    let folder = match folders::get(&state.db, msg.folder_id) {
        Ok(f) => f,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };
    let acc = match accounts::get(&state.db, folder.account_id) {
        Ok(a) => a,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };

    let (imap_pw, _) = resolve_secrets(&acc);
    let outcome = state.rt.block_on(async {
        if !imap_pw.is_empty() {
            let mut imap = mailcore::sync::imap::ImapSync::new(&acc);
            if imap.connect(&imap_pw).await.is_ok() {
                if let Ok(outcome) = imap.trash_message(&state.db, msg.id).await {
                    return format!("{outcome:?}");
                }
            }
        }
        let _ = messages::delete(&state.db, msg.id);
        "DeletedLocally".to_string()
    });

    let res = serde_json::json!({"ok": true, "outcome": outcome});
    to_jstring(&mut env, &res.to_string())
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeArchive(
    mut env: JNIEnv,
    _class: JClass,
    folder_id: jlong,
    uid: jlong,
) -> jstring {
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let msg = match messages::get_by_uid(&state.db, folder_id, uid as u32) {
        Ok(m) => m,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };
    let folder = match folders::get(&state.db, msg.folder_id) {
        Ok(f) => f,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };
    let acc = match accounts::get(&state.db, folder.account_id) {
        Ok(a) => a,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };

    let (imap_pw, _) = resolve_secrets(&acc);
    let outcome = state.rt.block_on(async {
        if !imap_pw.is_empty() {
            let mut imap = mailcore::sync::imap::ImapSync::new(&acc);
            if imap.connect(&imap_pw).await.is_ok() {
                if let Ok(outcome) = imap.archive_message(&state.db, msg.id).await {
                    return format!("{outcome:?}");
                }
            }
        }
        let _ = messages::delete(&state.db, msg.id);
        "ArchivedLocally".to_string()
    });

    let res = serde_json::json!({"ok": true, "outcome": outcome});
    to_jstring(&mut env, &res.to_string())
}

#[no_mangle]
pub extern "system" fn Java_mailclient_repo_NativeMailRepository_nativeOpenAttachment(
    mut env: JNIEnv,
    _class: JClass,
    attachment_id: jlong,
) -> jstring {
    let state = STATE.get().expect("NativeState not initialized").lock().unwrap();
    let att = match messages::get_attachment(&state.db, attachment_id) {
        Ok(a) => a,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };
    let msg = match messages::get(&state.db, att.message_id) {
        Ok(m) => m,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };
    let folder = match folders::get(&state.db, msg.folder_id) {
        Ok(f) => f,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };
    let acc = match accounts::get(&state.db, folder.account_id) {
        Ok(a) => a,
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            return to_jstring(&mut env, &err.to_string());
        }
    };

    let (imap_pw, _) = resolve_secrets(&acc);
    state.rt.block_on(async {
        let has_data = messages::attachment_has_data(&state.db, attachment_id).unwrap_or(false);
        if !has_data && !imap_pw.is_empty() {
            let mut imap = mailcore::sync::imap::ImapSync::new(&acc);
            if imap.connect(&imap_pw).await.is_ok() {
                let _ = imap.fetch_attachments(&state.db, msg.id).await;
            }
        }
    });

    let temp_dir = std::env::temp_dir().join("mailclient-attachments");
    let _ = std::fs::create_dir_all(&temp_dir);
    let safe_name = att.filename.as_deref().unwrap_or("attachment").replace(['/', '\\', '\0'], "_");
    let dest = temp_dir.join(format!("{}-{}-{}", att.message_id, att.id, safe_name));
    match messages::save_attachment_to_path(&state.db, attachment_id, &dest) {
        Ok(_) => {
            let path_str = dest.to_string_lossy();
            let res = serde_json::json!({"ok": true, "path": path_str});
            to_jstring(&mut env, &res.to_string())
        }
        Err(e) => {
            let err = serde_json::json!({"ok": false, "error": format!("{e}")});
            to_jstring(&mut env, &err.to_string())
        }
    }
}
