//! Sidebar rows: the painted fold of the folder tree.

use cxx_qt_lib::QString;

use crate::bridge::qobject;
use crate::bridge::{qstring, shared_db};

impl qobject::Bridge {
    pub fn sidebar_rows_json(&self, account_id: i64, expanded_json: &QString) -> QString {
        let expanded: Vec<i64> =
            serde_json::from_str(&expanded_json.to_string()).unwrap_or_default();
        let result = shared_db().and_then(|db| {
            mailcore::feed::sidebar_rows_json(db, account_id, &expanded).map_err(|e| e.to_string())
        });
        qstring(&result.unwrap_or_else(|e| {
            log::warn!("sidebar: cannot fold the folder tree: {e}");
            "[]".to_string()
        }))
    }
}
