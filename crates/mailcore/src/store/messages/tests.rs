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
fn count_and_min_uid_track_cache() {
    let (db, acc, f) = setup();
    assert_eq!(count_by_folder(&db, f).unwrap(), 0);
    assert_eq!(min_uid(&db, f).unwrap(), None);
    upsert(&db, &sample_new(acc, f, 5)).unwrap();
    upsert(&db, &sample_new(acc, f, 9)).unwrap();
    assert_eq!(count_by_folder(&db, f).unwrap(), 2);
    assert_eq!(min_uid(&db, f).unwrap(), Some(5));
}

#[test]
fn json_vec_helper() {
    assert_eq!(json_vec("[]").unwrap(), Vec::<String>::new());
    assert_eq!(json_vec("").unwrap(), Vec::<String>::new());
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
    // Resync replacement clears stale files.
    delete_attachments_for_message(&db, id).unwrap();
    assert!(list_attachments(&db, id).unwrap().is_empty());
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
    // IMAP UIDs define the server arrival order. The Date header is
    // sender-controlled, so a stale/future header must not reorder the
    // newest server window in the default list.
    assert_eq!(uids("date", true), vec![3, 2, 1]);
    assert_eq!(uids("date", false), vec![1, 2, 3]);
    assert_eq!(uids("from", true), vec![1, 3, 2]);
    assert_eq!(uids("subject", false), vec![2, 1, 3]);
    // Unknown fields fall back to date ordering.
    assert_eq!(uids("size", true), vec![3, 2, 1]);
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
    assert_eq!(
        crate::search::search(&db, acc, "Goodbye", 10)
            .unwrap()
            .len(),
        1
    );
}
