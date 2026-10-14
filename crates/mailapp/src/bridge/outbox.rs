use cxx_qt_lib::QString;

use crate::bridge::messages::file_url;
use crate::bridge::{qobject, qstring, shared_db};

// Thin adapter over `mailcore::outbox`: read the counts and rows, phrase the
// result. Retry needs no path of its own — the next sync flushes every
// submittable row, so "Retry now" is `sync_now`.

impl qobject::Bridge {
    pub fn outbox_json(&self, account_id: i64) -> QString {
        if account_id < 0 {
            return qstring("[]");
        }
        let result = shared_db()
            .map_err(|e| e.to_string())
            .and_then(|db| mailcore::outbox::list_json(db, account_id).map_err(|e| e.to_string()));
        qstring(&result.unwrap_or_else(|e| {
            log::warn!("outbox: cannot list queued sends: {e}");
            "[]".to_string()
        }))
    }

    pub fn outbox_status_json(&self, account_id: i64) -> QString {
        if account_id < 0 {
            return qstring("{}");
        }
        let result = shared_db().map_err(|e| e.to_string()).and_then(|db| {
            mailcore::outbox::status_json(db, account_id).map_err(|e| e.to_string())
        });
        qstring(&result.unwrap_or_else(|e| {
            log::warn!("outbox: cannot read status: {e}");
            "{}".to_string()
        }))
    }

    pub fn dismiss_outbox(&self, account_id: i64, id: i64) -> QString {
        let result = shared_db().map_err(|e| e.to_string()).and_then(|db| {
            mailcore::outbox::dismiss(db, account_id, id).map_err(|e| e.to_string())
        });
        qstring(&result.err().unwrap_or_default())
    }

    pub fn reschedule_outbox(&self, account_id: i64, id: i64, send_at: &QString) -> QString {
        let send_at = send_at.to_string();
        let result = shared_db().map_err(|e| e.to_string()).and_then(|db| {
            mailcore::outbox::reschedule(db, account_id, id, &send_at).map_err(|e| e.to_string())
        });
        qstring(&result.err().unwrap_or_default())
    }

    /// `outbox::take_for_edit` in the composer's draft shape (`openForDraft`:
    /// files as `file://` URLs, no server draft behind it), or `{error}`.
    pub fn outbox_edit(&self, account_id: i64, id: i64) -> QString {
        let result = shared_db().map_err(|e| e.to_string()).and_then(|db| {
            mailcore::outbox::take_for_edit(db, account_id, id, &std::env::temp_dir())
                .map_err(|e| e.to_string())
        });
        let json = match result {
            Ok(e) => serde_json::json!({
                "account_id": account_id,
                "draft_uid": -1,
                "from": e.from,
                "reply_to": e.reply_to,
                "to": e.to,
                "cc": e.cc,
                "bcc": e.bcc,
                "subject": e.subject,
                "body": e.editor_html,
                "attachments": e.files.iter().map(|f| serde_json::json!({
                    "path": file_url(std::path::Path::new(&f.path)),
                    "name": f.name,
                })).collect::<Vec<_>>(),
                "request_mdn": e.request_mdn,
                "request_dsn": e.request_dsn,
            }),
            Err(e) => serde_json::json!({ "error": e }),
        };
        qstring(&json.to_string())
    }

    pub fn send_now_outbox(&self, account_id: i64, id: i64) -> QString {
        let result = shared_db().map_err(|e| e.to_string()).and_then(|db| {
            mailcore::outbox::send_now(db, account_id, id).map_err(|e| e.to_string())
        });
        qstring(&result.err().unwrap_or_default())
    }
}
