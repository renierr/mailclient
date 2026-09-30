//! IMAP IDLE (RFC 2177): wait on the selected mailbox until the server
//! reports a change or the caller wants the connection back.

use std::future::Future;

use super::*;

/// Why [`ImapSession::idle`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleEnd {
    /// The server announced a change to the selected mailbox (new mail,
    /// expunge, flag change).
    Changed,
    /// The caller's `wake` future finished first. The IDLE was ended
    /// cleanly, so the session is ready for the next command.
    Woken,
}

impl ImapSession {
    /// IDLE on the selected mailbox until the server reports a change or
    /// `wake` resolves, then end it with `DONE` and wait for the tagged OK.
    ///
    /// Waiting is unbounded on purpose; only the entry and exit handshakes
    /// are bounded by [`COMMAND_TIMEOUT`]. The caller bounds the wait through
    /// `wake` (keep-alive tick, network change, stop). Server heartbeats
    /// (`* OK Still here`) are ignored, `BYE` is an error.
    pub async fn idle(&mut self, wake: impl Future<Output = ()>) -> Result<IdleEnd> {
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
                        event => log::debug!("imap: ignoring event while idling: {event:?}"),
                    }
                }
            }
        };

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
