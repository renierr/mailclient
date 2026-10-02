//! Local storage maintenance (settings section): stats, database export,
//! temp cleanup, cache trimming and attachment eviction. Every decision
//! lives in `mailcore::maintenance`; this only adapts to the bridge.

use std::pin::Pin;

use cxx_qt_lib::QString;
use mailcore::maintenance;

use crate::bridge::qobject;
use crate::bridge::worker::{spawn_job, JobRefresh};
use crate::bridge::{qstring, shared_db};

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

    // Trim and eviction are mass DELETE / UPDATE statements: on the GUI
    // thread a large cache freezes the window, and a sync on the net thread
    // could be writing rows for messages the trim removes. As jobs they run
    // in turn with sync on the one net thread.

    pub fn trim_cache(self: Pin<&mut Self>) -> QString {
        let (acc_id, folder_id) = (*self.current_account_id(), *self.current_folder_id());
        spawn_job(self, "Maintenance", move |db, _progress| async move {
            let keep = maintenance::TRIM_KEEP_PER_FOLDER;
            let removed = maintenance::trim_local_cache(db, keep).map_err(|e| e.to_string())?;
            // Rows are gone from the feeds: rebuild so the list, counts and
            // search stop showing deleted mail.
            Ok((
                maintenance::trim_status(removed, keep),
                Some(JobRefresh::feeds(acc_id, folder_id)),
            ))
        })
    }

    pub fn evict_attachments(self: Pin<&mut Self>) -> QString {
        spawn_job(self, "Maintenance", move |db, _progress| async move {
            let evicted = maintenance::evict_cached_attachments(db).map_err(|e| e.to_string())?;
            Ok((maintenance::evict_status(&evicted), None))
        })
    }
}
