//! IMAP IDLE (RFC 2177): wait on the selected mailbox until the server
//! reports a change or the caller wants the connection back.

use std::future::Future;
use std::time::{Duration, SystemTime};

use super::*;

/// Why [`ImapSession::idle`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleEnd {
    /// The server announced a change to the selected mailbox (new mail,
    /// expunge, flag change).
    Changed,
    /// The caller's `wake` future finished first, or its heartbeat hook
    /// asked to end. The IDLE was ended cleanly, so the session is ready for
    /// the next command.
    Woken,
}

/// What the server sent while the last IDLE waited, measured on the wall
/// clock: the monotonic clock stops while the phone sleeps, and a heartbeat
/// that wakes it would then look closer to the last one than it was.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdleStats {
    /// Untagged `OK` heartbeats (`* OK Still here`) received.
    pub heartbeats: u32,
    /// Average gap between heartbeats, counted from the start of the IDLE.
    /// `None` without heartbeats.
    pub heartbeat_every: Option<Duration>,
    /// How long the IDLE waited in total.
    pub idled: Duration,
}

impl IdleStats {
    fn measure(start: SystemTime, last_heartbeat: Option<SystemTime>, heartbeats: u32) -> Self {
        let since = |t: SystemTime| t.duration_since(start).unwrap_or_default();
        Self {
            heartbeats,
            heartbeat_every: last_heartbeat
                .filter(|_| heartbeats > 0)
                .map(|t| since(t) / heartbeats),
            idled: since(SystemTime::now()),
        }
    }
}

impl ImapSession {
    /// IDLE on the selected mailbox until the server reports a change or
    /// `wake` resolves, then end it with `DONE` and wait for the tagged OK.
    ///
    /// Waiting is unbounded on purpose; only the entry and exit handshakes
    /// are bounded by [`COMMAND_TIMEOUT`]. The caller bounds the wait through
    /// `wake` (keep-alive tick, network change, stop). Server heartbeats
    /// (`* OK Still here`) are counted in [`ImapSession::last_idle`] and
    /// passed to `on_heartbeat`, which ends the IDLE by returning `true` —
    /// the radio is awake for the heartbeat anyway, so a refresh then costs
    /// almost nothing. `BYE` is an error.
    pub async fn idle(
        &mut self,
        wake: impl Future<Output = ()>,
        mut on_heartbeat: impl FnMut() -> bool,
    ) -> Result<IdleEnd> {
        let tag = self.next_tag();
        let cmd = Command::new(tag.clone(), CommandBody::Idle)
            .map_err(|e| StoreError::InvalidInput(format!("invalid command: {e}")))?;
        self.client.enqueue_command(cmd);

        // Entry: until the server's continuation accepts the IDLE.
        loop {
            match self.next_event().await? {
                Event::IdleAccepted { .. } => break,
                Event::IdleRejected { status, .. } => {
                    return Err(StoreError::Network(format!("IDLE rejected: {status:?}")));
                }
                Event::StatusReceived {
                    status: Status::Bye(bye),
                } => return Err(bye_error(&bye.text)),
                _ => {}
            }
        }

        // Idling: the only phase without a timeout.
        let start = SystemTime::now();
        let mut heartbeats = 0u32;
        let mut last_heartbeat = None;
        tokio::pin!(wake);
        let end = loop {
            tokio::select! {
                () = &mut wake => break IdleEnd::Woken,
                event = self.stream.next(&mut self.client) => {
                    let event = event
                        .map_err(|e| StoreError::Network(format!("stream error: {e}")))?;
                    match event {
                        Event::DataReceived { data } if mailbox_changed(&data) => {
                            break IdleEnd::Changed;
                        }
                        Event::StatusReceived { status: Status::Bye(bye) } => {
                            return Err(bye_error(&bye.text));
                        }
                        Event::StatusReceived {
                            status: Status::Untagged(body),
                        } if body.kind == StatusKind::Ok => {
                            heartbeats += 1;
                            last_heartbeat = Some(SystemTime::now());
                            if on_heartbeat() {
                                break IdleEnd::Woken;
                            }
                        }
                        event => log::debug!("imap: ignoring event while idling: {event:?}"),
                    }
                }
            }
        };
        self.last_idle = IdleStats::measure(start, last_heartbeat, heartbeats);

        // Exit: DONE, then the tagged completion of the IDLE command.
        if self.client.set_idle_done().is_none() {
            return Err(StoreError::Network(
                "IDLE state lost before DONE".to_string(),
            ));
        }
        loop {
            match self.next_event().await? {
                Event::StatusReceived {
                    status: Status::Tagged(tagged),
                } if tagged.tag == tag => {
                    return match tagged.body.kind {
                        StatusKind::Ok => Ok(end),
                        kind => Err(StoreError::Network(format!(
                            "IDLE ended with {kind:?}: {}",
                            tagged.body.text
                        ))),
                    };
                }
                Event::StatusReceived {
                    status: Status::Bye(bye),
                } => return Err(bye_error(&bye.text)),
                _ => {}
            }
        }
    }

    /// Heartbeats seen during the most recent [`ImapSession::idle`].
    pub fn last_idle(&self) -> IdleStats {
        self.last_idle
    }

    /// One protocol event, bounded by [`COMMAND_TIMEOUT`].
    async fn next_event(&mut self) -> Result<Event> {
        tokio::time::timeout(COMMAND_TIMEOUT, self.stream.next(&mut self.client))
            .await
            .map_err(|_| StoreError::Network("timed out waiting for the server".to_string()))?
            .map_err(|e| StoreError::Network(format!("stream error: {e}")))
    }
}

/// Untagged data that means the selected mailbox is no longer what we
/// cached. Anything else (capability refreshes, quota notices) is noise.
fn mailbox_changed(data: &Data<'_>) -> bool {
    matches!(
        data,
        Data::Exists(_)
            | Data::Recent(_)
            | Data::Expunge(_)
            | Data::Fetch { .. }
            | Data::Vanished { .. }
    )
}

fn bye_error(text: &impl std::fmt::Display) -> StoreError {
    StoreError::Network(format!("server sent BYE: {text}"))
}
