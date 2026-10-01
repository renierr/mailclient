//! Local storage maintenance (settings section): stats, database export,
//! temp cleanup, cache trimming and attachment eviction. Every decision
//! lives in `mailcore::maintenance`; this only adapts to the bridge.

use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::maintenance;

use crate::bridge::qobject;
use crate::bridge::worker::spawn_job;
use crate::bridge::{push_feeds, qstring, shared_db};

/// Where viewer copies live for this frontend (same folder `open_attachment`
/// stages into).
fn temp_attachments_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("mailclient-attachments")
}

impl qobject::Bridge {
    pub fn maintenance_json(&self) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("{}");
        };
        let db_path_str = self.db_path().to_string();
        let db_path = std::path::Path::new(&db_path_str);
        maintenance::storage_stats_json(db, db_path, &temp_attachments_dir())
            .map_or_else(|_| qstring("{}"), |j| qstring(&j))
    }

    pub fn export_database(self: Pin<&mut Self>, path: &QString) -> QString {
        let path = path.to_string();
        spawn_job(self, "Maintenance", move |db, _progress| async move {
            let dest = maintenance::export_database(db, &path).map_err(|e| e.to_string())?;
            // Nothing the feeds show changed, so the list keeps its scroll
            // position and selection.
            Ok((format!("Database exported to {}", dest.display()), None))
        })
    }

    pub fn cleanup_temp(&self) -> QString {
        match maintenance::cleanup_temp_files(&temp_attachments_dir()) {
            Ok(done) => qstring(&maintenance::cleanup_status(&done)),
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn trim_cache(mut self: Pin<&mut Self>) -> QString {
        let db = match shared_db() {
            Ok(d) => d,
            Err(e) => return qstring(&e),
        };
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        match maintenance::trim_local_cache(db, maintenance::TRIM_KEEP_PER_FOLDER) {
            Ok(removed) => {
                // Rows are gone from the feeds: rebuild so the list, counts
                // and search stop showing deleted mail.
                push_feeds(&mut self, db, acc_id, folder_id);
                qstring(&maintenance::trim_status(
                    removed,
                    maintenance::TRIM_KEEP_PER_FOLDER,
                ))
            }
            Err(e) => qstring(&e.to_string()),
        }
    }

    pub fn evict_attachments(&self) -> QString {
        let Ok(db) = shared_db() else {
            return qstring("could not open the database");
        };
        match maintenance::evict_cached_attachments(db) {
            Ok(evicted) => qstring(&maintenance::evict_status(&evicted)),
            Err(e) => qstring(&e.to_string()),
        }
    }
}
