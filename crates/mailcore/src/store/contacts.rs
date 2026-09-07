//! `contacts` for address autocomplete.

use rusqlite::params;

use crate::db::Db;
use crate::error::Result;
use crate::models::Contact;
use crate::store::now;

/// Record having seen an address (insert or bump counter).
pub fn seen(db: &Db, address: &str, name: Option<&str>) -> Result<()> {
    let ts = now();
    db.conn().execute(
        "insert into contacts (address, name, times_seen, last_seen_at)
         values (?1, ?2, 1, ?3)
         on conflict (address) do update set
            name = coalesce(excluded.name, contacts.name),
            times_seen = contacts.times_seen + 1,
            last_seen_at = excluded.last_seen_at",
        params![address, name, ts],
    )?;
    Ok(())
}

/// Top matches for `prefix` (address or name), most-seen first.
pub fn suggest(db: &Db, prefix: &str, limit: u64) -> Result<Vec<Contact>> {
    let like = format!("{prefix}%");
    let mut stmt = db.conn().prepare(
        "select address, name, times_seen, last_seen_at from contacts
         where address like ?1 escape '\\' or name like ?1 escape '\\'
         order by times_seen desc, last_seen_at desc limit ?2",
    )?;
    let rows = stmt
        .query_map(params![like, limit as i64], |row| {
            Ok(Contact {
                address: row.get(0)?,
                name: row.get(1)?,
                times_seen: row.get::<_, i64>(2)? as u64,
                last_seen_at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seen_and_suggest() {
        let db = Db::open_in_memory().unwrap();
        seen(&db, "alice@example.com", Some("Alice")).unwrap();
        seen(&db, "alice@example.com", Some("Alice")).unwrap();
        seen(&db, "bob@example.com", None).unwrap();
        let s = suggest(&db, "a", 5).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].times_seen, 2);
        assert_eq!(suggest(&db, "", 5).unwrap().len(), 2);
    }
}
