//! [`ImapSession`]: the `imap-next` protocol verbs over Tokio.
//!
//! One command at a time, every wait bounded by a timeout; `BYE` is always
//! an error, never a hang. Higher-level orchestration lives in [`super::engine`].
//!
//! This file owns the connection itself -- tags, the command loop, login and
//! capability negotiation. The verbs built on top are grouped by what they
//! are for: [`mailbox`] reads a mailbox, [`mutate`] changes one, and
//! [`discovery`] asks what exists.

mod discovery;
mod mailbox;
mod mutate;

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

    /// Next command tag. The counter only grows — past `A9999` the tag simply
    /// gets longer, which is still a valid IMAP atom, so there is nothing to
    /// wrap and no way for the conversion to fail.
    fn next_tag(&mut self) -> Tag<'static> {
        self.tag_counter += 1;
        Tag::try_from(format!("A{:04}", self.tag_counter))
            .expect("A + decimal digits is always a valid IMAP tag")
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
}
