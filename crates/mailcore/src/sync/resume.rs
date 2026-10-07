//! Whether coming back to the foreground should sync an account.
//!
//! A phone flips between apps constantly; syncing on every return would hit
//! the server again and again for nothing. A return syncs only when the
//! account has not finished a sync within [`RESUME_SYNC_GRACE_MINUTES`] —
//! whoever ran it: the app, a background check or push. Only the resume
//! trigger asks this; the auto-sync timer, the startup sync and a manual
//! sync never do.

use chrono::{DateTime, Duration, Utc};
use rusqlite::OptionalExtension;

use crate::db::Db;

/// How long after a finished sync a return to the app skips its own.
pub const RESUME_SYNC_GRACE_MINUTES: i64 = 5;

/// True when a return to the foreground now should sync `account_id`: no
/// folder of it finished a sync within the grace period. A never-synced
/// account, or an unreadable timestamp, is due.
pub fn resume_sync_due(db: &Db, account_id: i64) -> bool {
    resume_sync_due_at(db, account_id, Utc::now())
}

fn resume_sync_due_at(db: &Db, account_id: i64, now: DateTime<Utc>) -> bool {
    let last: Option<String> = db
        .conn()
        .query_row(
            "select max(last_sync_at) from folders where account_id = ?1",
            [account_id],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
        .flatten();
    let Some(last) = last.and_then(|s| DateTime::parse_from_rfc3339(&s).ok()) else {
        return true;
    };
    now.signed_duration_since(last.with_timezone(&Utc))
        >= Duration::minutes(RESUME_SYNC_GRACE_MINUTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::FolderRole;
    use crate::store::{accounts, folders};

    fn synced_at(db: &Db, folder_id: i64, at: &str) {
        db.conn()
            .execute(
                "update folders set last_sync_at = ?1 where id = ?2",
                rusqlite::params![at, folder_id],
            )
            .unwrap();
    }

    #[test]
    fn grace_follows_the_latest_finished_sync() {
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create_for_test(&db, "a@example.com");
        let other = accounts::create_for_test(&db, "b@example.com");
        let now = DateTime::parse_from_rfc3339("2026-10-07T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        // Nothing synced yet.
        assert!(resume_sync_due_at(&db, acc, now));

        let inbox = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        let archive = folders::upsert(&db, acc, "Archive", "/", FolderRole::Archive).unwrap();
        assert!(resume_sync_due_at(&db, acc, now));

        // An old inbox sync, a fresh one of another folder: the fresh wins.
        synced_at(&db, inbox, "2026-10-07T11:00:00Z");
        assert!(resume_sync_due_at(&db, acc, now));
        synced_at(&db, archive, "2026-10-07T11:58:00Z");
        assert!(!resume_sync_due_at(&db, acc, now));

        // Exactly at the end of the grace period it is due again.
        synced_at(&db, archive, "2026-10-07T11:55:00Z");
        assert!(resume_sync_due_at(&db, acc, now));

        // Another account's sync does not count.
        let theirs = folders::upsert(&db, other, "INBOX", "/", FolderRole::Inbox).unwrap();
        synced_at(&db, theirs, "2026-10-07T11:59:00Z");
        assert!(resume_sync_due_at(&db, acc, now));
        assert!(!resume_sync_due_at(&db, other, now));

        // A garbled timestamp never blocks a sync.
        synced_at(&db, archive, "yesterday");
        synced_at(&db, inbox, "yesterday");
        assert!(resume_sync_due_at(&db, acc, now));
    }
}
