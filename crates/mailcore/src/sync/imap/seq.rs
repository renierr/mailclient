//! UID set helpers: [`SequenceSet`] building and QRESYNC VANISHED ranges.

use imap_types::sequence::{SeqOrUid, Sequence, SequenceSet};

use crate::error::{Result, StoreError};

/// Helper to extract all UIDs from a sequence set.
///
/// Only used for small sets (tests). Production VANISHED handling must use
/// [`vanished_ranges`] + range deletes instead: a server may report
/// `VANISHED 1:100000`, which would allocate ~100k entries here.
#[allow(dead_code)]
#[cfg(test)]
pub(crate) fn sequence_set_to_uids(set: &SequenceSet) -> Vec<u32> {
    let mut uids = Vec::new();
    for seq in set.0.as_ref() {
        match seq {
            Sequence::Single(SeqOrUid::Value(v)) => uids.push(v.get()),
            Sequence::Range(SeqOrUid::Value(a), SeqOrUid::Value(b)) => {
                let start = a.get().min(b.get());
                let end = a.get().max(b.get());
                // Guard against pathological ranges even in tests.
                if end.saturating_sub(start) > 100_000 {
                    continue;
                }
                for u in start..=end {
                    uids.push(u);
                }
            }
            _ => {}
        }
    }
    uids
}

/// Extract `(start, end)` ranges from a QRESYNC VANISHED sequence set without
/// expanding them into individual UIDs.
pub(crate) fn vanished_ranges(set: &SequenceSet) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for seq in set.0.as_ref() {
        match seq {
            Sequence::Single(SeqOrUid::Value(v)) => out.push((v.get(), v.get())),
            Sequence::Range(SeqOrUid::Value(a), SeqOrUid::Value(b)) => {
                out.push((a.get().min(b.get()), a.get().max(b.get())));
            }
            _ => {}
        }
    }
    out
}

/// Build a `SequenceSet` from a UID slice (deduped, sorted). Shared by all
/// FETCH/STORE/COPY call sites so the comma-join logic lives in one place.
pub(crate) fn uids_to_sequence_set(uids: &[u32]) -> Result<SequenceSet> {
    let mut clean: Vec<u32> = uids.to_vec();
    clean.sort_unstable();
    clean.dedup();
    let set_str = clean
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    SequenceSet::try_from(set_str.as_str())
        .map_err(|e| StoreError::InvalidInput(format!("invalid sequence set: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    use imap_types::sequence::SequenceSet;

    use crate::db::Db;
    use crate::models::FolderRole;
    use crate::store::{accounts, folders, messages};

    #[test]
    fn vanished_ranges_do_not_expand() {
        let set = SequenceSet::try_from("1:3,5,10:8").unwrap();
        assert_eq!(vanished_ranges(&set), vec![(1, 3), (5, 5), (8, 10)]);
        // Pathological range is skipped by the small-set helper …
        let huge = SequenceSet::try_from("1:200000").unwrap();
        assert!(sequence_set_to_uids(&huge).is_empty());
        // … and deleted by a single BETWEEN statement, not 200k rows.
        let db = Db::open_in_memory().unwrap();
        let acc = accounts::create(
            &db,
            &crate::models::NewAccount {
                name: "t".to_string(),
                email_address: "a@x.y".to_string(),
                from_name: String::new(),
                imap_host: "h".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "s".to_string(),
                smtp_port: 465,
                smtp_security: "tls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 60,
            },
        )
        .unwrap();
        let f = folders::upsert(&db, acc, "INBOX", "/", FolderRole::Inbox).unwrap();
        for uid in [1u32, 2, 3, 50, 100] {
            messages::upsert(&db, &messages::sample_new(acc, f, uid)).unwrap();
        }
        assert_eq!(messages::delete_by_uid_range(&db, f, 1, 3).unwrap(), 3);
        assert_eq!(messages::list_uids(&db, f).unwrap().len(), 2);
    }

    #[test]
    fn uids_to_sequence_set_sorts_and_dedups() {
        let set = uids_to_sequence_set(&[9, 3, 3, 7]).unwrap();
        assert_eq!(sequence_set_to_uids(&set), vec![3, 7, 9]);
    }
}
