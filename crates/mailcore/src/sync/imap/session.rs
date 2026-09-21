//! [`ImapSession`]: the `imap-next` protocol verbs over Tokio.
//!
//! One command at a time, every wait bounded by a timeout; `BYE` is always
//! an error, never a hang. Higher-level orchestration lives in [`super::engine`].

use core::num::{NonZeroU32, NonZeroU64};

use crate::error::{Result, StoreError};
use imap_next::{
    client::{Client, Event},
    stream::Stream,
};
use imap_types::{
    command::{Command, CommandBody, FetchModifier, SelectParameter},
    core::{Literal, Tag, Vec1},
    extensions::{binary::LiteralOrLiteral8, enable::CapabilityEnable},
    fetch::{MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName},
    flag::{Flag, FlagFetch, StoreResponse, StoreType},
    mailbox::Mailbox,
    response::{Code, Data, GreetingKind, Status, StatusKind},
    search::SearchKey,
    IntoStatic,
};

use super::{
    seq::{uids_to_sequence_set, vanished_ranges},
    types::{CommandResult, DiscoveredFolder, SelectResult, COMMAND_TIMEOUT},
};

/// Active IMAP session using `imap-next`.
pub struct ImapSession {
    stream: Stream,
    client: Client,
    tag_counter: u64,
    pub(crate) capabilities: Vec<String>,
    pub(crate) condstore_enabled: bool,
    pub(crate) qresync_enabled: bool,
}

impl ImapSession {
    /// Wrap a connected stream. The tag counter starts at 1 because a
    /// STARTTLS upgrade consumes tag `A0001` on the same client first.
    pub(crate) fn new(stream: Stream, client: Client) -> Self {
        Self {
            stream,
            client,
            tag_counter: 1,
            capabilities: Vec::new(),
            condstore_enabled: false,
            qresync_enabled: false,
        }
    }

    fn next_tag(&mut self) -> Tag<'static> {
        self.tag_counter += 1;
        Tag::try_from(format!("A{:04}", self.tag_counter))
            .expect("valid tag: A0000..A9999 with wrapping counter")
    }

    /// Read the initial server greeting, failing fast on `BYE` or timeout
    /// instead of spinning until TCP error.
    pub(crate) async fn read_greeting(stream: &mut Stream, client: &mut Client) -> Result<()> {
        loop {
            let event = tokio::time::timeout(COMMAND_TIMEOUT, stream.next(&mut *client))
                .await
                .map_err(|_| {
                    StoreError::Network("timed out waiting for server greeting".to_string())
                })?
                .map_err(|e| StoreError::Network(format!("greeting error: {e}")))?;
            match event {
                Event::GreetingReceived { greeting } => match greeting.kind {
                    GreetingKind::Bye => {
                        return Err(StoreError::Network(format!(
                            "server refused connection (BYE): {}",
                            greeting.text
                        )));
                    }
                    _ => {
                        if std::env::var("MAILCLIENT_IMAP_DEBUG").is_ok() {
                            log::warn!("imap S: greeting {greeting:?} (redact before sharing)");
                        }
                        return Ok(());
                    }
                },
                event => {
                    log::debug!("imap: unexpected greeting event: {event:?}");
                }
            }
        }
    }

    /// Execute a command and wait for its completion. `BYE` (untagged or
    /// tagged) is reported as an error immediately instead of looping on
    /// `stream.next()` forever; every wait is bounded by [`COMMAND_TIMEOUT`].
    pub(crate) async fn execute(&mut self, body: CommandBody<'static>) -> Result<CommandResult> {
        let tag = self.next_tag();
        let cmd = Command::new(tag.clone(), body)
            .map_err(|e| StoreError::InvalidInput(format!("invalid command: {e}")))?;
        let handle = self.client.enqueue_command(cmd);

        let mut collected_data = Vec::new();
        let mut untagged_statuses = Vec::new();

        loop {
            let event = tokio::time::timeout(COMMAND_TIMEOUT, self.stream.next(&mut self.client))
                .await
                .map_err(|_| {
                    StoreError::Network(format!("timed out waiting for server reply to {tag:?}"))
                })?
                .map_err(|e| StoreError::Network(format!("stream error: {e}")))?;

            match event {
                Event::CommandSent { handle: h, .. } if h == handle => {
                    // command sent
                }
                Event::CommandRejected {
                    handle: h, status, ..
                } if h == handle => {
                    return Err(StoreError::Network(format!("command rejected: {status:?}")));
                }
                Event::DataReceived { data } => {
                    collected_data.push(data.into_static());
                }
                Event::StatusReceived { status } => match status {
                    Status::Tagged(tagged) if tagged.tag == tag => match tagged.body.kind {
                        StatusKind::Ok => {
                            return Ok(CommandResult {
                                data: collected_data,
                                untagged_statuses,
                                status: Status::Tagged(tagged).into_static(),
                            });
                        }
                        StatusKind::No | StatusKind::Bad => {
                            return Err(StoreError::Network(format!(
                                "server returned {:?}: {}",
                                tagged.body.kind, tagged.body.text
                            )));
                        }
                    },
                    Status::Tagged(tagged) => {
                        // Tagged completion for some other command (e.g. a
                        // stale STARTTLS tag): never silently swallow.
                        log::debug!("imap: ignoring foreign tagged status: {tagged:?}");
                    }
                    Status::Untagged(untagged) => {
                        untagged_statuses.push(untagged.into_static());
                    }
                    Status::Bye(bye) => {
                        return Err(StoreError::Network(format!(
                            "server sent BYE during {tag:?}: {}",
                            bye.text
                        )));
                    }
                },
                Event::ContinuationRequestReceived { .. }
                | Event::AuthenticateContinuationRequestReceived { .. }
                | Event::AuthenticateStatusReceived { .. }
                | Event::AuthenticateStarted { .. }
                | Event::IdleCommandSent { .. }
                | Event::IdleAccepted { .. }
                | Event::IdleRejected { .. }
                | Event::IdleDoneSent { .. } => {
                    log::debug!("imap: unexpected auth/idle event during {tag:?}: {event:?}");
                }
                _ => {
                    log::debug!("imap: ignoring event during {tag:?}: {event:?}");
                }
            }
        }
    }

    /// Authenticate with LOGIN.
    pub async fn login(&mut self, user: &str, pass: &str) -> Result<()> {
        let body = CommandBody::login(user.to_string(), pass.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("login args invalid: {e}")))?;
        self.execute(body).await?;
        Ok(())
    }

    /// Query CAPABILITY.
    pub async fn capability(&mut self) -> Result<Vec<String>> {
        let res = self.execute(CommandBody::Capability).await?;
        let mut caps = Vec::new();
        for d in res.data {
            if let Data::Capability(c) = d {
                for cap in c.as_ref() {
                    caps.push(cap.to_string());
                }
            }
        }
        let check_code = |code: &Option<Code<'_>>, caps: &mut Vec<String>| {
            if let Some(Code::Capability(c)) = code {
                for cap in c.as_ref() {
                    caps.push(cap.to_string());
                }
            }
        };
        for s in &res.untagged_statuses {
            check_code(&s.code, &mut caps);
        }
        if let Status::Tagged(t) = &res.status {
            check_code(&t.body.code, &mut caps);
        }
        caps.sort();
        caps.dedup();
        self.capabilities = caps.clone();
        Ok(caps)
    }

    /// Check if a capability is present (case-insensitive).
    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case(cap))
    }

    /// Try to enable CONDSTORE and QRESYNC if advertised.
    pub async fn enable_extensions(&mut self) -> Result<()> {
        let has_enable = self.has_capability("enable");
        let has_condstore = self.has_capability("condstore");
        let has_qresync = self.has_capability("qresync");

        if !has_condstore && !has_qresync {
            return Ok(());
        }

        if has_enable {
            let mut enable_caps = Vec::new();
            if has_condstore {
                if let Ok(cap) = CapabilityEnable::try_from("CONDSTORE") {
                    enable_caps.push(cap);
                }
            }
            if has_qresync {
                if let Ok(cap) = CapabilityEnable::try_from("QRESYNC") {
                    enable_caps.push(cap);
                }
            }

            if let Ok(caps) = Vec1::try_from(enable_caps) {
                let body = CommandBody::enable(caps)
                    .map_err(|e| StoreError::InvalidInput(format!("enable args invalid: {e}")))?;
                if let Ok(res) = self.execute(body).await {
                    for d in res.data {
                        if let Data::Enabled { capabilities } = d {
                            for cap in &capabilities {
                                let s = cap.to_string().to_ascii_lowercase();
                                if s == "condstore" {
                                    self.condstore_enabled = true;
                                }
                                if s == "qresync" {
                                    self.qresync_enabled = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        // If CONDSTORE was advertised without ENABLE (RFC 4551), it is activated via SELECT.
        if has_condstore && !self.condstore_enabled {
            self.condstore_enabled = true;
        }

        log::info!(
            "imap: extensions enabled: condstore={}, qresync={}",
            self.condstore_enabled,
            self.qresync_enabled
        );
        Ok(())
    }

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

    /// STORE flags (+FLAGS / -FLAGS).
    pub async fn uid_store_flags(
        &mut self,
        uids: &[u32],
        op: StoreType,
        flags: Vec<Flag<'static>>,
    ) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let sequence_set = uids_to_sequence_set(uids)?;

        let body = CommandBody::store(sequence_set, op, StoreResponse::Silent, flags, true)
            .map_err(|e| StoreError::InvalidInput(format!("store args: {e}")))?;

        self.execute(body).await?;
        Ok(())
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

    /// UID COPY.
    pub async fn uid_copy(&mut self, uids: &[u32], dest: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let sequence_set = uids_to_sequence_set(uids)?;
        let mailbox = Mailbox::try_from(dest.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {dest}: {e}")))?;
        let body = CommandBody::copy(sequence_set, mailbox, true)
            .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
        self.execute(body).await?;
        Ok(())
    }

    /// UID MOVE (or fallback copy + deleted + expunge).
    pub async fn uid_move(&mut self, uids: &[u32], dest: &str) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        let has_move = self
            .capabilities
            .iter()
            .any(|c| c.eq_ignore_ascii_case("move"));

        let sequence_set = uids_to_sequence_set(uids)?;
        let mailbox = Mailbox::try_from(dest.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {dest}: {e}")))?;

        if has_move {
            let body = CommandBody::Move {
                sequence_set: sequence_set.clone(),
                mailbox: mailbox.clone(),
                uid: true,
            };
            if let Err(e) = self.execute(body).await {
                log::warn!("imap: UID MOVE failed ({e}), falling back to COPY + STORE + EXPUNGE");
                let body = CommandBody::copy(sequence_set, mailbox, true)
                    .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
                self.execute(body).await?;
                self.uid_store_flags(uids, StoreType::Add, vec![Flag::Deleted])
                    .await?;
                self.uid_expunge(uids).await?;
            }
        } else {
            let body = CommandBody::copy(sequence_set, mailbox, true)
                .map_err(|e| StoreError::InvalidInput(format!("copy args: {e}")))?;
            self.execute(body).await?;
            self.uid_store_flags(uids, StoreType::Add, vec![Flag::Deleted])
                .await?;
            self.uid_expunge(uids).await?;
        }
        Ok(())
    }

    /// EXPUNGE — removes **every** `\Deleted` message in the mailbox.
    ///
    /// Only correct when that is genuinely what is meant. To destroy
    /// specific messages use [`Self::uid_expunge`], which does not reach
    /// past them.
    pub async fn expunge(&mut self) -> Result<()> {
        self.execute(CommandBody::Expunge).await?;
        Ok(())
    }

    /// UID EXPUNGE (RFC 4315): destroy only `uids`, among those flagged
    /// `\Deleted`.
    ///
    /// Plain `EXPUNGE` takes the whole mailbox with it, so a message another
    /// client had flagged `\Deleted` but not yet expunged — its own pending
    /// delete, still undoable on its side — was destroyed as a side effect of
    /// us deleting something unrelated. Servers without UIDPLUS leave no
    /// alternative, and there the wide behaviour is what "delete" has to
    /// mean; everywhere else this stays inside the selection the user made.
    pub async fn uid_expunge(&mut self, uids: &[u32]) -> Result<()> {
        if uids.is_empty() {
            return Ok(());
        }
        if !self.has_capability("uidplus") {
            log::debug!("imap: no UIDPLUS, falling back to mailbox-wide EXPUNGE");
            return self.expunge().await;
        }
        let sequence_set = uids_to_sequence_set(uids)?;
        match self.execute(CommandBody::ExpungeUid { sequence_set }).await {
            Ok(_) => Ok(()),
            Err(e) => {
                // Advertised but refused: the messages are already flagged
                // `\Deleted`, so leaving them is the wrong outcome too.
                log::warn!("imap: UID EXPUNGE failed ({e}), falling back to EXPUNGE");
                self.expunge().await
            }
        }
    }

    /// APPEND.
    pub async fn append(
        &mut self,
        folder: &str,
        raw: &[u8],
        flags: Vec<Flag<'static>>,
    ) -> Result<()> {
        let mailbox = Mailbox::try_from(folder.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {folder}: {e}")))?;
        let literal = Literal::try_from(raw.to_vec())
            .map_err(|e| StoreError::InvalidInput(format!("literal error: {e}")))?;

        let body = CommandBody::Append {
            mailbox,
            flags,
            date: None,
            message: LiteralOrLiteral8::Literal(literal),
        };
        self.execute(body).await?;
        Ok(())
    }

    /// CREATE mailbox.
    pub async fn create_folder(&mut self, folder: &str) -> Result<()> {
        let mailbox = Mailbox::try_from(folder.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("invalid mailbox {folder}: {e}")))?;
        let body = CommandBody::create(mailbox)
            .map_err(|e| StoreError::InvalidInput(format!("create args: {e}")))?;
        self.execute(body).await?;
        Ok(())
    }

    /// LIST folders.
    pub async fn list(&mut self, reference: &str, pattern: &str) -> Result<Vec<DiscoveredFolder>> {
        let body = CommandBody::list(reference.to_string(), pattern.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("list args: {e}")))?;

        let res = self.execute(body).await?;
        let mut out = Vec::new();

        for d in res.data {
            if let Data::List {
                items,
                delimiter,
                mailbox,
            } = d
            {
                let name = match mailbox {
                    Mailbox::Inbox => "INBOX".to_string(),
                    Mailbox::Other(o) => String::from_utf8_lossy(o.as_ref()).to_string(),
                };
                let delim = delimiter
                    .map(|d| d.inner().to_string())
                    .unwrap_or_else(|| "/".to_string());
                out.push(DiscoveredFolder {
                    name,
                    delimiter: delim,
                    attributes: items.into_iter().map(|i| i.into_static()).collect(),
                });
            }
        }
        Ok(out)
    }

    /// LSUB folders.
    pub async fn lsub(&mut self, reference: &str, pattern: &str) -> Result<Vec<DiscoveredFolder>> {
        let body = CommandBody::lsub(reference.to_string(), pattern.to_string())
            .map_err(|e| StoreError::InvalidInput(format!("lsub args: {e}")))?;

        let res = self.execute(body).await?;
        let mut out = Vec::new();

        for d in res.data {
            if let Data::Lsub {
                items,
                delimiter,
                mailbox,
            } = d
            {
                let name = match mailbox {
                    Mailbox::Inbox => "INBOX".to_string(),
                    Mailbox::Other(o) => String::from_utf8_lossy(o.as_ref()).to_string(),
                };
                let delim = delimiter
                    .map(|d| d.inner().to_string())
                    .unwrap_or_else(|| "/".to_string());
                out.push(DiscoveredFolder {
                    name,
                    delimiter: delim,
                    attributes: items.into_iter().map(|i| i.into_static()).collect(),
                });
            }
        }
        Ok(out)
    }

    /// NOOP health check.
    pub async fn noop(&mut self) -> Result<()> {
        self.execute(CommandBody::Noop).await?;
        Ok(())
    }

    /// NAMESPACE (RFC 2342, best effort). Returns `(personal, other, shared)`
    /// prefix strings. Servers that don't implement it, or answer with a
    /// shape the codec can't model, yield empty vecs — discovery simply
    /// covers fewer branches. Never fails sync.
    pub async fn namespace(&mut self) -> Result<(Vec<String>, Vec<String>, Vec<String>)> {
        let res = match self.execute(CommandBody::Namespace).await {
            Ok(r) => r,
            Err(e) => {
                log::debug!("imap: NAMESPACE unsupported, skipping: {e}");
                return Ok((Vec::new(), Vec::new(), Vec::new()));
            }
        };
        let mut personal = Vec::new();
        let mut other = Vec::new();
        let mut shared = Vec::new();
        for d in res.data {
            if let Data::Namespace {
                personal: p,
                other: o,
                shared: s,
            } = d
            {
                fn prefix(ns: &imap_types::extensions::namespace::Namespace<'_>) -> String {
                    String::from_utf8_lossy(ns.prefix.clone().into_inner().as_ref()).into_owned()
                }
                personal.extend(p.iter().map(prefix));
                other.extend(o.iter().map(prefix));
                shared.extend(s.iter().map(prefix));
            }
        }
        Ok((personal, other, shared))
    }
}
