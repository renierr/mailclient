use cxx_qt_lib::QString;

use crate::bridge::{qobject, qstring, shared_db};

// Thin adapter over `mailcore::outbox`: read the counts and rows, phrase the
// result. Retry needs no path of its own — the next sync flushes every
// submittable row, so "Retry now" is `sync_now`.

impl qobject::Bridge {
    pub fn outbox_json(&self) -> QString {
        let id = *self.current_account_id();
        if id < 0 {
            return qstring("[]");
        }
        let result = shared_db()
            .map_err(|e| e.to_string())
            .and_then(|db| mailcore::outbox::list_json(db, id).map_err(|e| e.to_string()));
        qstring(&result.unwrap_or_else(|e| {
            log::warn!("outbox: cannot list queued sends: {e}");
            "[]".to_string()
        }))
    }

    pub fn outbox_status_json(&self) -> QString {
        let id = *self.current_account_id();
        if id < 0 {
            return qstring("{}");
        }
        let result = shared_db()
            .map_err(|e| e.to_string())
            .and_then(|db| mailcore::outbox::status_json(db, id).map_err(|e| e.to_string()));
        qstring(&result.unwrap_or_else(|e| {
            log::warn!("outbox: cannot read status: {e}");
            "{}".to_string()
        }))
    }

    pub fn dismiss_outbox(&self, id: i64) -> QString {
        let account_id = *self.current_account_id();
        let result = shared_db().map_err(|e| e.to_string()).and_then(|db| {
            mailcore::outbox::dismiss(db, account_id, id).map_err(|e| e.to_string())
        });
        match result {
            Ok(1) => qstring(""),
            Ok(_) => qstring("message is no longer available"),
            Err(e) => qstring(&e),
        }
    }
}
