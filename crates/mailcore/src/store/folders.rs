//! CRUD for `folders` (IMAP folder tree per account).

use rusqlite::{params, OptionalExtension};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Folder, FolderRole};
use crate::store::{now, opt_bool};

fn row_to_folder(row: &rusqlite::Row<'_>) -> rusqlite::Result<Folder> {
    let role: String = row.get(4)?;
    Ok(Folder {
        id: row.get(0)?,
        account_id: row.get(1)?,
        path: row.get(2)?,
        delimiter: row.get(3)?,
        role: FolderRole::parse_role(&role),
        uid_validity: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        uid_next: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
        subscribed: opt_bool(row.get::<_, i64>(7)?),
        last_sync_at: row.get(8)?,
    })
}

const COLS: &str = "id, account_id, path, delimiter, role, uid_validity,
    uid_next, subscribed, last_sync_at";

/// Insert or update a folder identified by `(account_id, path)` (IMAP LIST sync).
pub fn upsert(
    db: &Db,
    account_id: i64,
    path: &str,
    delimiter: &str,
    role: FolderRole,
) -> Result<i64> {
    let ts = now();
    db.conn().execute(
        "insert into folders (account_id, path, delimiter, role, subscribed, created_at, updated_at)
         values (?1, ?2, ?3, ?4, 1, ?5, ?5)
         on conflict (account_id, path) do update set
            delimiter = excluded.delimiter,
            role = excluded.role,
            updated_at = excluded.updated_at",
        params![account_id, path, delimiter, role.as_str(), ts],
    )?;
    let id: i64 = db.conn().query_row(
        "select id from folders where account_id = ?1 and path = ?2",
        params![account_id, path],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// List folders of one account: inbox first, then special roles in a fixed
/// order, then custom folders alphabetically.
pub fn list_by_account(db: &Db, account_id: i64) -> Result<Vec<Folder>> {
    let mut stmt = db.conn().prepare(&format!(
        "select {COLS} from folders where account_id = ?1 order by path"
    ))?;
    let mut rows = stmt
        .query_map([account_id], row_to_folder)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.sort_by(|a, b| {
        role_weight(a.role).cmp(&role_weight(b.role)).then_with(|| {
            a.path
                .to_ascii_lowercase()
                .cmp(&b.path.to_ascii_lowercase())
        })
    });
    Ok(rows)
}

fn role_weight(role: FolderRole) -> u8 {
    match role {
        FolderRole::Inbox => 0,
        FolderRole::Drafts => 1,
        FolderRole::Sent => 2,
        FolderRole::Archive => 3,
        FolderRole::Junk => 4,
        FolderRole::Trash => 5,
        FolderRole::Custom => 6,
    }
}

/// Fetch one folder by `(account_id, path)`.
pub fn get_by_path(db: &Db, account_id: i64, path: &str) -> Result<Folder> {
    db.conn()
        .query_row(
            &format!("select {COLS} from folders where account_id = ?1 and path = ?2"),
            params![account_id, path],
            row_to_folder,
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("folder {path}")))
}

/// Fetch one folder by id.
pub fn get(db: &Db, id: i64) -> Result<Folder> {
    db.conn()
        .query_row(
            &format!("select {COLS} from folders where id = ?1"),
            [id],
            row_to_folder,
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("folder {id}")))
}

/// Record UIDVALIDITY/UIDNEXT + sync timestamp after a successful SELECT.
pub fn set_sync_state(db: &Db, id: i64, uid_validity: u32, uid_next: u32) -> Result<()> {
    let ts = now();
    let n = db.conn().execute(
        "update folders set uid_validity = ?1, uid_next = ?2, last_sync_at = ?3,
            updated_at = ?3 where id = ?4",
        params![uid_validity as i64, uid_next as i64, ts, id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound(format!("folder {id}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAccount;
    use crate::store::accounts;

    fn mk_account(db: &Db) -> i64 {
        accounts::create(
            db,
            &NewAccount {
                name: "a".to_string(),
                email_address: "a@x.y".to_string(),
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
        .unwrap()
    }

    #[test]
    fn upsert_list_sync_state() {
        let db = Db::open_in_memory().unwrap();
        let acc = mk_account(&db);
        let inbox = upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        let again = upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        assert_eq!(inbox, again);
        upsert(&db, acc, "INBOX.Sent", ".", FolderRole::Sent).unwrap();
        assert_eq!(list_by_account(&db, acc).unwrap().len(), 2);
        set_sync_state(&db, inbox, 123, 456).unwrap();
        let f = get(&db, inbox).unwrap();
        assert_eq!(f.uid_validity, Some(123));
        assert_eq!(f.uid_next, Some(456));
        assert!(f.last_sync_at.is_some());
    }

    #[test]
    fn inbox_first_then_roles_then_custom() {
        let db = Db::open_in_memory().unwrap();
        let acc = mk_account(&db);
        for (path, role) in [
            ("Trash", FolderRole::Trash),
            ("INBOX.Work", FolderRole::Custom),
            ("Sent", FolderRole::Sent),
            ("INBOX", FolderRole::Inbox),
            ("Archive", FolderRole::Archive),
            ("Drafts", FolderRole::Drafts),
            ("Junk", FolderRole::Junk),
            ("Zebra", FolderRole::Custom),
        ] {
            upsert(&db, acc, path, "/", role).unwrap();
        }
        let names: Vec<String> = list_by_account(&db, acc)
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(
            names,
            vec![
                "INBOX",
                "Drafts",
                "Sent",
                "Archive",
                "Junk",
                "Trash",
                "INBOX.Work",
                "Zebra"
            ]
        );
    }
}
