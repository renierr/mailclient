//! Reading a mailbox: SELECT, the two UID FETCH forms and UID SEARCH.
//!
//! Each extension-bearing variant falls back to the plain form when the
//! server rejects it, and turns the extension off for the rest of the
//! session rather than retrying it on every folder.

use super::*;

impl ImapSession {
    /// SELECT a folder, with optional QRESYNC parameters and automatic fallback.
    pub async fn select(
        &mut self,
        path: &str,
        qresync: Option<(u32, u64)>,
    ) -> Result<SelectResult> {
        let mailbox = Mailbox::try_from(path.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {path}: {e}")))?;

        let (body, has_extension) = if self.qresync_enabled && qresync.is_some_and(|(_, m)| m > 0) {
            let (validity, modseq) = qresync.unwrap();
            let nz_val = NonZeroU32::new(validity);
            let nz_mod = NonZeroU64::new(modseq);
            if let (Some(v), Some(m)) = (nz_val, nz_mod) {
                let param = SelectParameter::QResync {
                    uid_validity: v,
                    mod_sequence_value: m,
                    known_uids: None,
                    seq_match_data: None,
                };
                (
                    CommandBody::Select {
                        mailbox: mailbox.clone(),
                        parameters: vec![param],
                    },
                    true,
                )
            } else {
                (
                    CommandBody::Select {
                        mailbox: mailbox.clone(),
                        parameters: vec![SelectParameter::CondStore],
                    },
                    true,
                )
            }
        } else if self.condstore_enabled {
            (
                CommandBody::Select {
                    mailbox: mailbox.clone(),
                    parameters: vec![SelectParameter::CondStore],
                },
                true,
            )
        } else {
            (
                CommandBody::select(mailbox.clone()).map_err(|e| {
                    StoreError::InvalidInput(format!("invalid mailbox {path}: {e}"))
                })?,
                false,
            )
        };

        let res = match self.execute(body).await {
            Ok(r) => r,
            Err(e) if has_extension => {
                log::warn!(
                    "imap: SELECT with extension failed ({e}), falling back to standard SELECT"
                );
                self.condstore_enabled = false;
                self.qresync_enabled = false;
                let fallback = CommandBody::select(mailbox).map_err(|e| {
                    StoreError::InvalidInput(format!("invalid mailbox {path}: {e}"))
                })?;
                self.execute(fallback).await?
            }
            Err(e) => return Err(e),
        };

        let mut out = SelectResult::default();

        for d in res.data {
            match d {
                Data::Exists(n) => out.exists = n,
                Data::Vanished {
                    earlier: _,
                    known_uids,
                } => {
                    out.vanished.extend(vanished_ranges(&known_uids));
                }
                _ => {}
            }
        }

        let mut check_code = |code: &Option<Code<'_>>| {
            if let Some(c) = code {
                match c {
                    Code::UidValidity(v) => out.uid_validity = Some(v.get()),
                    Code::UidNext(n) => out.uid_next = Some(n.get()),
                    Code::HighestModSeq(m) => out.highest_modseq = Some(m.get()),
                    _ => {}
                }
            }
        };

        for s in &res.untagged_statuses {
            check_code(&s.code);
        }
        if let Status::Tagged(t) = &res.status {
            check_code(&t.body.code);
        }

        Ok(out)
    }

    /// Fetch flags with optional CONDSTORE `CHANGEDSINCE`.
    pub async fn uid_fetch_flags_changesince(
        &mut self,
        uids: &[u32],
        modseq: u64,
    ) -> Result<Vec<(u32, Vec<Flag<'static>>, Option<u64>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let sequence_set = uids_to_sequence_set(uids)?;

        let (macro_or_item_names, modifiers) = if self.condstore_enabled && modseq > 0 {
            match NonZeroU64::new(modseq) {
                Some(nz) => (
                    MacroOrMessageDataItemNames::from(vec![
                        MessageDataItemName::Uid,
                        MessageDataItemName::Flags,
                        MessageDataItemName::ModSeq,
                    ]),
                    vec![FetchModifier::ChangedSince(nz)],
                ),
                None => (
                    MacroOrMessageDataItemNames::from(vec![
                        MessageDataItemName::Uid,
                        MessageDataItemName::Flags,
                    ]),
                    Vec::new(),
                ),
            }
        } else {
            (
                MacroOrMessageDataItemNames::from(vec![
                    MessageDataItemName::Uid,
                    MessageDataItemName::Flags,
                ]),
                Vec::new(),
            )
        };

        let has_modifiers = !modifiers.is_empty();
        let body = CommandBody::Fetch {
            sequence_set: sequence_set.clone(),
            macro_or_item_names,
            uid: true,
            modifiers,
        };

        let res = match self.execute(body).await {
            Ok(r) => r,
            Err(e) if has_modifiers => {
                log::warn!(
                    "imap: UID FETCH CHANGEDSINCE failed ({e}), falling back to standard UID FETCH"
                );
                self.condstore_enabled = false;
                let fallback = CommandBody::Fetch {
                    sequence_set,
                    macro_or_item_names: MacroOrMessageDataItemNames::from(vec![
                        MessageDataItemName::Uid,
                        MessageDataItemName::Flags,
                    ]),
                    uid: true,
                    modifiers: Vec::new(),
                };
                self.execute(fallback).await?
            }
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();

        for d in res.data {
            if let Data::Fetch { items, .. } = d {
                let mut uid = 0u32;
                let mut flags = Vec::new();
                let mut msg_modseq = None;

                for item in items.as_ref() {
                    match item {
                        MessageDataItem::Uid(u) => uid = u.get(),
                        MessageDataItem::Flags(f) => {
                            for flag_fetch in f {
                                if let FlagFetch::Flag(flag) = flag_fetch {
                                    flags.push(flag.clone().into_static());
                                }
                            }
                        }
                        MessageDataItem::ModSeq(m) => msg_modseq = Some(m.get()),
                        _ => {}
                    }
                }
                if uid > 0 {
                    out.push((uid, flags, msg_modseq));
                }
            }
        }

        Ok(out)
    }

    /// Fetch full messages (UID, FLAGS, and raw RFC822 bodies).
    pub async fn uid_fetch_messages(
        &mut self,
        uids: &[u32],
    ) -> Result<Vec<(u32, Vec<Flag<'static>>, Vec<u8>)>> {
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let sequence_set = uids_to_sequence_set(uids)?;

        let body = CommandBody::Fetch {
            sequence_set,
            macro_or_item_names: MacroOrMessageDataItemNames::from(vec![
                MessageDataItemName::Uid,
                MessageDataItemName::Flags,
                MessageDataItemName::BodyExt {
                    section: None,
                    partial: None,
                    peek: true,
                },
            ]),
            uid: true,
            modifiers: Vec::new(),
        };

        let res = self.execute(body).await?;
        let mut out = Vec::new();

        for d in res.data {
            if let Data::Fetch { items, .. } = d {
                let mut uid = 0u32;
                let mut flags = Vec::new();
                let mut raw_body = Vec::new();

                for item in items.as_ref() {
                    match item {
                        MessageDataItem::Uid(u) => uid = u.get(),
                        MessageDataItem::Flags(f) => {
                            for flag_fetch in f {
                                if let FlagFetch::Flag(flag) = flag_fetch {
                                    flags.push(flag.clone().into_static());
                                }
                            }
                        }
                        MessageDataItem::BodyExt { data, .. } => {
                            if let Some(bytes) = data.0.as_ref().map(|s| s.as_ref()) {
                                raw_body = bytes.to_vec();
                            }
                        }
                        MessageDataItem::Rfc822(data) => {
                            if let Some(bytes) = data.0.as_ref().map(|s| s.as_ref()) {
                                raw_body = bytes.to_vec();
                            }
                        }
                        _ => {}
                    }
                }
                if uid > 0 {
                    out.push((uid, flags, raw_body));
                }
            }
        }

        Ok(out)
    }

    /// UID SEARCH with criteria.
    pub async fn uid_search(&mut self, criteria: Vec1<SearchKey<'static>>) -> Result<Vec<u32>> {
        let body = CommandBody::search(None, criteria, true);
        let res = self.execute(body).await?;
        let mut uids = Vec::new();
        for d in res.data {
            if let Data::Search(found, _) = d {
                uids.extend(found.iter().map(|n| n.get()));
            }
        }
        Ok(uids)
    }
}
