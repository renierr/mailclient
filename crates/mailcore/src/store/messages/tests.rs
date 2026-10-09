//! Tests for the `messages` / `attachments` store.
//!
//! Split out of the parent once they outgrew it (see AGENT.md, "File size
//! & where tests live"). Still a `#[cfg(test)]` submodule of it, so
//! `super::*` reaches its private items exactly as before.

use super::*;
use crate::models::{FolderRole, NewAccount};
use crate::store::{accounts, folders};

fn setup() -> (Db, i64, i64) {
    let db = Db::open_in_memory().unwrap();
    let acc = accounts::create(
        &db,
        &NewAccount {
            name: "a".to_string(),
            email_address: "a@x.y".to_string(),
            from_name: String::new(),
            imap_host: "h".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "u".to_string(),
            smtp_host: "h".to_string(),
            smtp_port: 465,
            smtp_security: "tls".to_string(),
            smtp_username: "u".to_string(),
            auth_vault_key: "k".to_string(),
            check_interval_secs: 60,
        },
    )
    .unwrap();
    let f = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
    (db, acc, f)
}

#[test]
fn upsert_list_flags_unread_delete() {
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 1)).unwrap();
    // Same UID upserts to the same row.
    assert_eq!(upsert(&db, &sample_new(acc, f, 1)).unwrap(), id);
    upsert(&db, &sample_new(acc, f, 2)).unwrap();
    assert_eq!(list_by_folder(&db, f, 10, 0).unwrap().len(), 2);
    assert_eq!(count_unread(&db, f).unwrap(), 2);
    set_flags(&db, id, true, true).unwrap();
    let m = get(&db, id).unwrap();
    assert!(m.is_read && m.is_starred);
    assert_eq!(m.to_addrs, vec!["bob@example.com".to_string()]);
    assert_eq!(count_unread(&db, f).unwrap(), 1);
    delete(&db, id).unwrap();
    assert!(matches!(get(&db, id), Err(StoreError::NotFound(_))));
}

#[test]
fn is_listed_hides_pending_moves_and_deleted_rows() {
    use crate::store::pending_moves::{self, PendingAction};
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 1)).unwrap();
    assert!(is_listed(&db, f, 1).unwrap());
    assert!(!is_listed(&db, f, 2).unwrap());
    pending_moves::queue(
        &db,
        &[id],
        PendingAction::Trash,
        None,
        "b",
        "2099-01-01T00:00:00Z",
    )
    .unwrap();
    assert!(!is_listed(&db, f, 1).unwrap());
    pending_moves::cancel_batch(&db, "b").unwrap();
    assert!(is_listed(&db, f, 1).unwrap());
    delete(&db, id).unwrap();
    assert!(!is_listed(&db, f, 1).unwrap());
}

#[test]
fn local_flag_change_queues_for_push_then_clears() {
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 1)).unwrap();
    let other = upsert(&db, &sample_new(acc, f, 2)).unwrap();
    // A freshly synced message owes the server nothing.
    assert!(list_flags_dirty(&db, acc).unwrap().is_empty());

    // Reading a message locally (the click path) queues the flag push
    // instead of doing it inline.
    set_flags(&db, id, true, false).unwrap();
    let dirty = list_flags_dirty(&db, acc).unwrap();
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].id, id);
    assert!(dirty[0].is_read);

    // Once pushed, the row is settled and stays out of the queue.
    assert!(clear_flags_dirty(&db, id, true, false).unwrap());
    assert!(list_flags_dirty(&db, acc).unwrap().is_empty());

    // Starring queues too, and only the touched row.
    set_flags(&db, other, false, true).unwrap();
    let dirty = list_flags_dirty(&db, acc).unwrap();
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty[0].id, other);
    assert!(dirty[0].is_starred);
}

#[test]
fn a_toggle_during_the_push_is_not_swallowed_by_the_clear() {
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 1)).unwrap();

    // The click that starts the push.
    set_flags(&db, id, true, false).unwrap();
    let in_flight = list_flags_dirty(&db, acc).unwrap().remove(0);

    // The user clicks again while the push is still on the network.
    set_flags(&db, id, false, false).unwrap();

    // The push lands and reports the state it actually sent. The newer
    // click must survive it.
    assert!(!clear_flags_dirty(&db, id, in_flight.is_read, in_flight.is_starred).unwrap());
    let still_dirty = list_flags_dirty(&db, acc).unwrap();
    assert_eq!(still_dirty.len(), 1);
    assert!(!still_dirty[0].is_read, "the newer unread state was lost");

    // The next round pushes that state and does settle the row.
    assert!(clear_flags_dirty(&db, id, false, false).unwrap());
    assert!(list_flags_dirty(&db, acc).unwrap().is_empty());
}

#[test]
fn server_flag_refresh_skips_dirty_rows() {
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 1)).unwrap();
    set_flags(&db, id, true, true).unwrap();
    set_flags_by_uid(&db, acc, f, 1, false, false, false).unwrap();
    let m = get(&db, id).unwrap();
    assert!(m.is_read && m.is_starred);
    assert_eq!(list_flags_dirty(&db, acc).unwrap().len(), 1);

    let mut again = sample_new(acc, f, 1);
    again.is_read = false;
    again.is_starred = false;
    again.subject = Some("resync".to_string());
    upsert(&db, &again).unwrap();
    let m = get(&db, id).unwrap();
    assert!(m.is_read && m.is_starred);
    assert_eq!(m.subject.as_deref(), Some("resync"));
}

#[test]
fn count_and_max_uid_track_cache() {
    let (db, acc, f) = setup();
    assert_eq!(count_by_folder(&db, f).unwrap(), 0);
    assert_eq!(max_uid(&db, f).unwrap(), None);
    upsert(&db, &sample_new(acc, f, 5)).unwrap();
    upsert(&db, &sample_new(acc, f, 9)).unwrap();
    assert_eq!(count_by_folder(&db, f).unwrap(), 2);
    assert_eq!(max_uid(&db, f).unwrap(), Some(9));
}

#[test]
fn json_vec_helper() {
    use crate::store::{json_vec, json_vec_logged};
    assert_eq!(json_vec("[]").unwrap(), Vec::<String>::new());
    assert_eq!(json_vec("").unwrap(), Vec::<String>::new());
    assert!(json_vec("not json").is_err());
    assert_eq!(
        json_vec_logged("not json", "message", "to_addrs", 1),
        Vec::<String>::new()
    );
}

#[test]
fn read_clean_moves_only_the_read_flag_of_clean_rows() {
    // B9: the Trash sweep used `set_flags_by_uid(.., true, false, false)`,
    // which also cleared a starred row's star and a draft's flag.
    let (db, acc, f) = setup();
    let mut starred = sample_new(acc, f, 1);
    starred.is_read = false;
    starred.is_starred = true;
    upsert(&db, &starred).unwrap();
    let mut plain = sample_new(acc, f, 2);
    plain.is_read = false;
    upsert(&db, &plain).unwrap();
    let mut kept_unread = sample_new(acc, f, 3);
    kept_unread.is_read = false;
    upsert(&db, &kept_unread).unwrap();
    // The user marked 3 unread locally: a pending change of its own.
    set_read_many_by_uids(&db, f, &[3], false).unwrap();

    assert_eq!(set_read_clean_by_uids(&db, f, &[1, 3], false).unwrap(), 1);
    let one = get_by_uid(&db, f, 1).unwrap();
    assert!(one.is_read && one.is_starred, "the star survives");
    assert!(
        !get_by_uid(&db, f, 3).unwrap().is_read,
        "a dirty row is left alone"
    );
    let dirty: Vec<u32> = list_flags_dirty(&db, acc)
        .unwrap()
        .iter()
        .map(|m| m.uid)
        .collect();
    assert_eq!(dirty, vec![3], "a pushed STORE leaves nothing to push");

    // B10: when the STORE failed, the row is queued for the flag push.
    assert_eq!(set_read_clean_by_uids(&db, f, &[2], true).unwrap(), 1);
    assert!(get_by_uid(&db, f, 2).unwrap().is_read);
    let mut dirty: Vec<u32> = list_flags_dirty(&db, acc)
        .unwrap()
        .iter()
        .map(|m| m.uid)
        .collect();
    dirty.sort_unstable();
    assert_eq!(dirty, vec![2, 3]);
}

#[test]
fn a_corrupt_address_column_still_reads_the_row() {
    // The row stays readable (one bad column must not hide the message);
    // the column reads as empty with a warning instead of an error.
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 4)).unwrap();
    db.conn()
        .execute(
            "update messages set to_addrs = 'not json' where id = ?1",
            [id],
        )
        .unwrap();
    let m = get_by_uid(&db, f, 4).unwrap();
    assert!(m.to_addrs.is_empty());
}

#[test]
fn attachment_blob_roundtrips_and_saves_to_disk() {
    use crate::models::NewAttachment;
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 60)).unwrap();
    // Metadata-only listing carries no bytes (feed stays cheap).
    let aid = add_attachment(
        &db,
        id,
        &NewAttachment {
            filename: Some("notes.txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            content_id: None,
            size: 16,
            data: Some(b"hello attachment".to_vec()),
            is_inline: false,
        },
    )
    .unwrap();
    let listed = list_attachments(&db, id).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].filename.as_deref(), Some("notes.txt"));
    assert_eq!(listed[0].size, 16);
    assert!(listed[0].data.is_none());
    assert!(!listed[0].is_inline);
    assert!(attachment_has_data(&db, aid).unwrap());
    // Full fetch carries the bytes; save writes them back to disk.
    let full = get_attachment(&db, aid).unwrap();
    assert_eq!(full.data.as_deref(), Some(b"hello attachment".as_slice()));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.txt");
    let n = save_attachment_to_path(&db, aid, &dest).unwrap();
    assert_eq!(n, 16);
    assert_eq!(std::fs::read(&dest).unwrap(), b"hello attachment");
}

#[test]
fn attachment_meta_row_reports_no_data() {
    use crate::models::NewAttachment;
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 61)).unwrap();
    // What background sync stores: real name/size, no bytes.
    let aid = add_attachment(
        &db,
        id,
        &NewAttachment {
            filename: Some("doc.pdf".to_string()),
            mime_type: Some("application/pdf".to_string()),
            content_id: None,
            size: 11,
            data: None,
            is_inline: false,
        },
    )
    .unwrap();
    assert!(!attachment_has_data(&db, aid).unwrap());
    let listed = list_attachments(&db, id).unwrap();
    assert_eq!(listed[0].size, 11);
    // Saving without bytes is a clean NotFound (the bridge downloads first).
    let dir = tempfile::tempdir().unwrap();
    assert!(save_attachment_to_path(&db, aid, &dir.path().join("x")).is_err());
    // Only the flag flips on refresh — never read/star state.
    set_has_attachments(&db, id, true).unwrap();
    assert!(get(&db, id).unwrap().has_attachments);
}

#[test]
fn bulk_flag_updates_preserve_the_other_flag() {
    let (db, acc, f) = setup();
    for uid in [1u32, 2, 3] {
        upsert(&db, &sample_new(acc, f, uid)).unwrap();
    }
    assert_eq!(
        set_read_many_by_uids(&db, f, &[1, 2, 2, 1], true).unwrap(),
        2
    );
    assert_eq!(count_unread(&db, f).unwrap(), 1);
    // Unknown UIDs are ignored, empty is a no-op.
    assert_eq!(set_read_many_by_uids(&db, f, &[99], true).unwrap(), 0);
    assert_eq!(set_read_many_by_uids(&db, f, &[], true).unwrap(), 0);
    // Starring keeps the read state untouched.
    assert_eq!(set_star_many_by_uids(&db, f, &[1, 3], true).unwrap(), 2);
    assert!(get_by_uid(&db, f, 1).unwrap().is_read);
    assert!(get_by_uid(&db, f, 1).unwrap().is_starred);
    assert!(!get_by_uid(&db, f, 2).unwrap().is_starred);
    // Both bulk paths queue for the next server push.
    assert_eq!(list_flags_dirty(&db, acc).unwrap().len(), 3);
}

#[test]
fn sorted_listing_orders_by_field_and_direction() {
    let (db, acc, f) = setup();
    let mut a = sample_new(acc, f, 1);
    a.from_addr = Some("zeta@example.com".to_string());
    a.subject = Some("Banana".to_string());
    a.date = Some("2026-09-01T10:00:00+00:00".to_string());
    upsert(&db, &a).unwrap();
    let mut b = sample_new(acc, f, 2);
    b.from_addr = Some("alpha@example.com".to_string());
    b.subject = Some("Apple".to_string());
    b.date = Some("2026-09-03T10:00:00+00:00".to_string());
    upsert(&db, &b).unwrap();
    let mut c = sample_new(acc, f, 3);
    c.from_addr = Some("mid@example.com".to_string());
    c.subject = Some("Cherry".to_string());
    c.date = Some("2026-09-02T10:00:00+00:00".to_string());
    upsert(&db, &c).unwrap();

    // Ordering is exercised through the listing the UI actually pages
    // with; both share `folder_sort_clause`.
    let uids = |field: &str, desc: bool| {
        list_compact_by_folder_sorted(&db, f, 10, 0, field, desc)
            .unwrap()
            .into_iter()
            .map(|m| m.uid)
            .collect::<Vec<_>>()
    };
    // Date follows the shown date, not the UID: a mail moved in later gets a
    // higher UID but keeps its place among its dates.
    assert_eq!(uids("date", true), vec![2, 3, 1]);
    assert_eq!(uids("date", false), vec![1, 3, 2]);
    assert_eq!(uids("from", true), vec![1, 3, 2]);
    assert_eq!(uids("subject", false), vec![2, 1, 3]);
    // Unknown fields fall back to date ordering.
    assert_eq!(uids("size", true), vec![2, 3, 1]);

    // An old mail moved in (new high UID) sorts by its date; undated rows go
    // last in both directions, equal dates fall back to the UID.
    let mut moved = sample_new(acc, f, 50);
    moved.date = Some("2026-08-01T10:00:00+00:00".to_string());
    upsert(&db, &moved).unwrap();
    let mut undated = sample_new(acc, f, 60);
    undated.date = None;
    upsert(&db, &undated).unwrap();
    let mut twin = sample_new(acc, f, 70);
    twin.date = Some("2026-09-02T10:00:00+00:00".to_string());
    upsert(&db, &twin).unwrap();
    assert_eq!(uids("date", true), vec![2, 70, 3, 1, 50, 60]);
    assert_eq!(uids("date", false), vec![50, 1, 3, 70, 2, 60]);
}

#[test]
fn bulk_delete_removes_only_the_folder_uids() {
    let (db, acc, f) = setup();
    for uid in [1u32, 2, 3] {
        upsert(&db, &sample_new(acc, f, uid)).unwrap();
    }
    assert_eq!(delete_many_by_uids(&db, f, &[1, 3, 3]).unwrap(), 2);
    assert_eq!(count_by_folder(&db, f).unwrap(), 1);
    let compact = list_compact_by_folder_sorted(&db, f, 10, 0, "date", true).unwrap();
    assert_eq!(compact.len(), 1);
    assert_eq!(compact[0].uid, 2);
    assert_eq!(compact[0].subject.as_deref(), Some("Hello"));
    assert!(!compact[0].is_read);
    assert!(!compact[0].is_starred);
}

fn updated_at_of(db: &Db, folder_id: i64, uid: u32) -> String {
    db.conn()
        .query_row(
            "select updated_at from messages where folder_id = ?1 and uid = ?2",
            params![folder_id, uid],
            |r| r.get(0),
        )
        .unwrap()
}

/// Rows in the FTS shadow table: every re-index appends to it, so this is a
/// direct read on whether the update trigger fired.
fn fts_rows(db: &Db) -> i64 {
    db.conn()
        .query_row("select count(*) from messages_fts_data", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn counts_by_account_agrees_with_the_per_folder_queries() {
    let (db, acc, f) = setup();
    let other = folders::upsert(&db, acc, "Archive", "/", FolderRole::Archive).unwrap();
    let id = upsert(&db, &sample_new(acc, f, 1)).unwrap();
    upsert(&db, &sample_new(acc, f, 2)).unwrap();
    upsert(&db, &sample_new(acc, other, 3)).unwrap();
    set_flags(&db, id, true, false).unwrap();

    let counts = counts_by_account(&db, acc).unwrap();
    assert_eq!(counts[&f].total, count_by_folder(&db, f).unwrap());
    assert_eq!(counts[&f].unread, count_unread(&db, f).unwrap());
    assert_eq!(counts[&other].total, 1);
    assert_eq!(counts[&other].unread, 1);
}

#[test]
fn an_empty_folder_is_simply_absent_from_the_counts() {
    let (db, acc, _f) = setup();
    let empty = folders::upsert(&db, acc, "Spam", "/", FolderRole::Custom).unwrap();
    assert!(!counts_by_account(&db, acc).unwrap().contains_key(&empty));
}

#[test]
fn unchanged_server_flags_touch_nothing() {
    let (db, acc, f) = setup();
    upsert(&db, &sample_new(acc, f, 1)).unwrap();
    let before = updated_at_of(&db, f, 1);
    let index_before = fts_rows(&db);

    // What sync does for every message in the window on every pass.
    set_flags_by_uid(&db, acc, f, 1, false, false, false).unwrap();

    assert_eq!(updated_at_of(&db, f, 1), before);
    assert_eq!(
        fts_rows(&db),
        index_before,
        "flag no-op re-indexed the body"
    );
}

#[test]
fn a_real_flag_change_still_lands_without_re_indexing() {
    let (db, acc, f) = setup();
    upsert(&db, &sample_new(acc, f, 1)).unwrap();
    let index_before = fts_rows(&db);

    set_flags_by_uid(&db, acc, f, 1, true, false, false).unwrap();

    assert!(get_by_uid(&db, f, 1).unwrap().is_read);
    assert_eq!(
        fts_rows(&db),
        index_before,
        "a flag change re-indexed the body"
    );
}

#[test]
fn changing_indexed_text_does_re_index() {
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 1)).unwrap();
    let index_before = fts_rows(&db);

    db.conn()
        .execute(
            "update messages set subject = 'Goodbye' where id = ?1",
            [id],
        )
        .unwrap();

    assert!(fts_rows(&db) > index_before);
    // The index now carries the new subject, not the old one.
    let hits = crate::feed::search_json(&db, acc, "Goodbye", 10, "").unwrap();
    assert_eq!(
        serde_json::from_str::<Vec<serde_json::Value>>(&hits)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_deferred_read_then_write_fails_where_immediate_succeeds() {
    // Why `replace_attachments` opens an Immediate transaction and reads
    // *before* it: a deferred transaction that has already read holds a
    // snapshot, and the first write then has to upgrade it. A commit from any
    // other connection in between makes that upgrade fail immediately with
    // SQLITE_BUSY_SNAPSHOT, and the busy handler does not cover it -- the GUI
    // thread and the net thread both own a connection, and the `--sync-once`
    // CLI shares the file.
    use rusqlite::TransactionBehavior;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upgrade.sqlite");
    let db = Db::open(&path).unwrap();
    let other = rusqlite::Connection::open(&path).unwrap();
    other.execute_batch("pragma busy_timeout = 5000;").unwrap();

    // The competing write has to change a page the snapshot read: a statement
    // writing nothing (`where 1 = 0`) or setting a column to itself lets the
    // upgrade succeed, which quietly makes this test pass for the wrong reason.
    let bump = "update schema_meta set value = 'A' where key = 'version'";
    let read = "select value from schema_meta where key = 'version'";

    // Deferred: the read lands inside the transaction, then the other
    // connection commits, then the first write needs the upgrade.
    {
        let tx = other.unchecked_transaction().unwrap();
        let seen: String = tx.query_row(read, [], |r| r.get(0)).unwrap();
        assert_eq!(seen, crate::db::migrations::SCHEMA_VERSION.to_string());
        db.conn().execute(bump, []).unwrap();
        let err = tx
            .execute(bump, [])
            .expect_err("a deferred read should not upgrade after a competing commit");
        // SQLITE_BUSY_SNAPSHOT specifically: a plain SQLITE_BUSY would mean the
        // busy handler gave up, which is a different failure.
        assert!(
            matches!(
                &err,
                rusqlite::Error::SqliteFailure(e, _) if e.extended_code == 517
            ),
            "expected SQLITE_BUSY_SNAPSHOT (517), got {err:?}"
        );
        let stale = tx.query_row(read, [], |r| r.get::<_, String>(0)).unwrap();
        // Still the value from before the commit: the snapshot really is held.
        assert_eq!(
            stale, seen,
            "the snapshot was not held, so the race is not exercised"
        );
        let _ = tx.rollback();
    }

    // Immediate: the write lock is taken when the transaction opens, so the
    // same read-then-write sequence goes through once the other has committed.
    {
        let tx = rusqlite::Transaction::new_unchecked(&other, TransactionBehavior::Immediate)
            .expect("immediate should start once the other writer committed");
        let seen: String = tx.query_row(read, [], |r| r.get(0)).unwrap();
        assert_eq!(seen, "A");
        tx.execute(
            "update schema_meta set value = 'B' where key = 'version'",
            [],
        )
        .expect("an immediate transaction reads and then writes without a snapshot upgrade");
        tx.commit().unwrap();
    }
}

#[test]
fn a_uid_list_past_the_sqlite_variable_limit_still_applies() {
    // SQLite caps bound parameters at 32,766 per statement and the leading
    // folder / updated_at bindings already use some, so one statement carrying
    // a whole large folder's `in (?)` list failed outright -- a folder-wide
    // bulk read, archive, or a `push_due_moves` group is enough to reach it.
    // The rows sit on the chunk boundary (UID_CHUNK is 900), so a boundary
    // bug in the loop drops one of them.
    let (db, acc, f) = setup();
    let uids: [u32; 5] = [1, 900, 901, 32_766, 40_000];
    for uid in uids {
        upsert(&db, &sample_new(acc, f, uid)).unwrap();
    }
    let all: Vec<u32> = (1..=40_000).collect();
    // One statement would be 40,003 parameters here.
    assert_eq!(set_read_many_by_uids(&db, f, &all, true).unwrap(), 5);
    assert_eq!(count_unread(&db, f).unwrap(), 0);
    assert_eq!(set_star_many_by_uids(&db, f, &all, true).unwrap(), 5);
    // Both bulk paths still queue for the server push.
    assert_eq!(list_flags_dirty(&db, acc).unwrap().len(), 5);
    // And the chunk loop still reports real row counts, not a per-chunk total.
    assert_eq!(delete_many_by_uids(&db, f, &all).unwrap(), 5);
    // The multi-chunk transaction was committed, not left open.
    assert!(db.conn().is_autocommit());
}

#[test]
fn a_missing_attachment_is_not_found_not_a_database_error() {
    // `query_row` without `.optional()` leaked the raw `Query returned no
    // rows`, which the reader shows as a database error. It must not report
    // `Ok(false)` either: `false` means "not cached yet" and a caller would
    // start a download for an attachment that does not exist.
    use crate::models::NewAttachment;
    let (db, acc, f) = setup();
    let id = upsert(&db, &sample_new(acc, f, 71)).unwrap();
    let aid = add_attachment(
        &db,
        id,
        &NewAttachment {
            filename: Some("real.txt".to_string()),
            mime_type: Some("text/plain".to_string()),
            content_id: None,
            size: 4,
            data: Some(b"data".to_vec()),
            is_inline: false,
        },
    )
    .unwrap();
    assert!(attachment_has_data(&db, aid).unwrap());
    // One that simply is not there.
    let err = attachment_has_data(&db, aid + 9_999).unwrap_err();
    assert!(
        matches!(err, StoreError::NotFound(_)),
        "a missing attachment should be NotFound, got {err:?}"
    );
    // And it never says "database" to a reader, whatever else happens.
    assert!(!err.to_string().contains("database"), "{err}");
}
