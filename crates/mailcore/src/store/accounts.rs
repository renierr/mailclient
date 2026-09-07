//! CRUD for `accounts`.

use rusqlite::{params, OptionalExtension};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Account, NewAccount};
use crate::store::now;

fn row_to_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        name: row.get(1)?,
        email_address: row.get(2)?,
        imap_host: row.get(3)?,
        imap_port: row.get::<_, i64>(4)? as u16,
        imap_security: row.get(5)?,
        imap_username: row.get(6)?,
        smtp_host: row.get(7)?,
        smtp_port: row.get::<_, i64>(8)? as u16,
        smtp_security: row.get(9)?,
        smtp_username: row.get(10)?,
        auth_vault_key: row.get(11)?,
        check_interval_secs: row.get::<_, i64>(12)? as u64,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

const COLS: &str = "id, name, email_address, imap_host, imap_port, imap_security,
    imap_username, smtp_host, smtp_port, smtp_security, smtp_username,
    auth_vault_key, check_interval_secs, created_at, updated_at";

/// Insert a new account, returning its row id.
pub fn create(db: &Db, a: &NewAccount) -> Result<i64> {
    let ts = now();
    db.conn().execute(
        "insert into accounts (name, email_address, imap_host, imap_port,
            imap_security, imap_username, smtp_host, smtp_port, smtp_security,
            smtp_username, auth_vault_key, check_interval_secs, created_at, updated_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            a.name,
            a.email_address,
            a.imap_host,
            a.imap_port as i64,
            a.imap_security,
            a.imap_username,
            a.smtp_host,
            a.smtp_port as i64,
            a.smtp_security,
            a.smtp_username,
            a.auth_vault_key,
            a.check_interval_secs as i64,
            ts,
            ts,
        ],
    )?;
    Ok(db.conn().last_insert_rowid())
}

/// Fetch one account by id.
pub fn get(db: &Db, id: i64) -> Result<Account> {
    db.conn()
        .query_row(
            &format!("select {COLS} from accounts where id = ?1"),
            [id],
            row_to_account,
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound(format!("account {id}")))
}

/// List all accounts ordered by name.
pub fn list(db: &Db) -> Result<Vec<Account>> {
    let mut stmt = db
        .conn()
        .prepare(&format!("select {COLS} from accounts order by name"))?;
    let rows = stmt
        .query_map([], row_to_account)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Update connection fields of an existing account (vault key untouched).
pub fn update_connection(db: &Db, id: i64, a: &NewAccount) -> Result<()> {
    let ts = super::now();
    let n = db.conn().execute(
        "update accounts set name = ?1, email_address = ?2, imap_host = ?3,
            imap_port = ?4, imap_security = ?5, imap_username = ?6,
            smtp_host = ?7, smtp_port = ?8, smtp_security = ?9,
            smtp_username = ?10, check_interval_secs = ?11, updated_at = ?12
         where id = ?13",
        params![
            a.name,
            a.email_address,
            a.imap_host,
            a.imap_port as i64,
            a.imap_security,
            a.imap_username,
            a.smtp_host,
            a.smtp_port as i64,
            a.smtp_security,
            a.smtp_username,
            a.check_interval_secs as i64,
            ts,
            id,
        ],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound(format!("account {id}")));
    }
    Ok(())
}

/// Delete an account (folders/messages/queue cascade).
pub fn delete(db: &Db, id: i64) -> Result<()> {
    let n = db
        .conn()
        .execute("delete from accounts where id = ?1", [id])?;
    if n == 0 {
        return Err(StoreError::NotFound(format!("account {id}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn sample() -> NewAccount {
        NewAccount {
            name: "Work".to_string(),
            email_address: "user@example.com".to_string(),
            imap_host: "imap.example.com".to_string(),
            imap_port: 993,
            imap_security: "tls".to_string(),
            imap_username: "user".to_string(),
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 465,
            smtp_security: "tls".to_string(),
            smtp_username: "user".to_string(),
            auth_vault_key: "vault:work".to_string(),
            check_interval_secs: 300,
        }
    }

    #[test]
    fn create_get_list_delete() {
        let db = Db::open_in_memory().unwrap();
        let id = create(&db, &sample()).unwrap();
        let a = get(&db, id).unwrap();
        assert_eq!(a.email_address, "user@example.com");
        assert_eq!(a.imap_port, 993);
        assert_eq!(list(&db).unwrap().len(), 1);
        delete(&db, id).unwrap();
        assert!(list(&db).unwrap().is_empty());
        assert!(matches!(get(&db, id), Err(StoreError::NotFound(_))));
    }
}
