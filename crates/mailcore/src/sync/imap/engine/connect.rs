//! Connection lifecycle: dialling, TLS (implicit or STARTTLS), login, and
//! the two ways a session ends.
//!
//! Sessions are pooled by the caller, so "is this one still usable?" is
//! part of the lifecycle too -- see [`ImapSync::is_healthy`].

use super::*;

impl ImapSync {
    /// Connect and authenticate with the server.
    pub async fn connect(&mut self, password: &str) -> Result<()> {
        if self.session.is_some() {
            return Ok(());
        }
        if !self.endpoint.implicit_tls && !self.endpoint.starttls {
            let sec = self.account.imap_security.trim().to_ascii_lowercase();
            if sec != "plain" && sec != "none" {
                return Err(StoreError::InvalidInput(format!(
                    "refusing plaintext IMAP connection to {}: set security to 'tls' or 'starttls'",
                    self.endpoint.addr
                )));
            }
        }

        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(&self.endpoint.addr))
            .await
            .map_err(|_| {
                StoreError::Network(format!(
                    "connect timed out after {:?}: {}",
                    CONNECT_TIMEOUT, self.endpoint.addr
                ))
            })?
            .map_err(|e| StoreError::Network(format!("connect {}: {e}", self.endpoint.addr)))?;

        let (stream, client) = if self.endpoint.implicit_tls {
            let tls_connector = build_tls_connector()?;
            let server_name = server_name_for(&self.endpoint.host)?;
            let tls =
                tokio::time::timeout(CONNECT_TIMEOUT, tls_connector.connect(server_name, tcp))
                    .await
                    .map_err(|_| {
                        StoreError::Network(format!(
                            "TLS handshake timed out after {:?} with {}",
                            CONNECT_TIMEOUT, self.endpoint.addr
                        ))
                    })?
                    .map_err(|e| {
                        StoreError::Network(format!(
                            "TLS handshake failed with {}: {e}",
                            self.endpoint.addr
                        ))
                    })?;
            let mut stream = Stream::tls(tokio_rustls::TlsStream::Client(tls));
            let mut client = Client::new(Options::default());
            ImapSession::read_greeting(&mut stream, &mut client).await?;
            (stream, client)
        } else {
            let mut stream = Stream::insecure(tcp);
            let mut client = Client::new(Options::default());
            ImapSession::read_greeting(&mut stream, &mut client).await?;

            if self.endpoint.starttls {
                let tag = Tag::try_from("A0001")
                    .map_err(|e| StoreError::InvalidInput(format!("starttls tag invalid: {e}")))?;
                let handle = client.enqueue_command(
                    Command::new(tag.clone(), CommandBody::StartTLS).map_err(|e| {
                        StoreError::InvalidInput(format!("starttls command invalid: {e}"))
                    })?,
                );
                loop {
                    let event = tokio::time::timeout(COMMAND_TIMEOUT, stream.next(&mut client))
                        .await
                        .map_err(|_| {
                            StoreError::Network("timed out waiting for STARTTLS reply".to_string())
                        })?
                        .map_err(|e| StoreError::Network(format!("STARTTLS stream error: {e}")))?;
                    match event {
                        Event::StatusReceived {
                            status: Status::Tagged(tagged),
                        } => {
                            if tagged.tag == tag {
                                if tagged.body.kind == StatusKind::Ok {
                                    break;
                                } else {
                                    return Err(StoreError::Network(format!(
                                        "STARTTLS rejected: {}",
                                        tagged.body.text
                                    )));
                                }
                            } else {
                                log::debug!("imap: ignoring foreign tagged status: {tagged:?}");
                            }
                        }
                        Event::StatusReceived {
                            status: Status::Bye(bye),
                        } => {
                            return Err(StoreError::Network(format!(
                                "server sent BYE during STARTTLS: {}",
                                bye.text
                            )));
                        }
                        Event::CommandRejected {
                            handle: h, status, ..
                        } if h == handle => {
                            return Err(StoreError::Network(format!(
                                "STARTTLS rejected: {status:?}"
                            )));
                        }
                        _ => {}
                    }
                }

                let tcp_stream: TcpStream = stream.into();
                let tls_connector = build_tls_connector()?;
                let server_name = server_name_for(&self.endpoint.host)?;
                let tls = tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    tls_connector.connect(server_name, tcp_stream),
                )
                .await
                .map_err(|_| {
                    StoreError::Network(format!(
                        "STARTTLS handshake timed out after {:?} with {}",
                        CONNECT_TIMEOUT, self.endpoint.addr
                    ))
                })?
                .map_err(|e| {
                    StoreError::Network(format!(
                        "STARTTLS handshake failed with {}: {e}",
                        self.endpoint.addr
                    ))
                })?;
                let stream = Stream::tls(tokio_rustls::TlsStream::Client(tls));
                (stream, client)
            } else {
                (stream, client)
            }
        };

        let mut session = ImapSession::new(stream, client);

        let username = if self.account.imap_username.is_empty() {
            &self.account.email_address
        } else {
            &self.account.imap_username
        };

        session.login(username, password).await?;
        session.capability().await?;
        let _ = session.enable_extensions().await;

        self.session = Some(session);
        log::info!(
            "imap: connected and authenticated for {}",
            self.account.email_address
        );
        Ok(())
    }

    /// Best-effort async LOGOUT (sends `LOGOUT`, waits briefly for `BYE`,
    /// then drops the stream either way). Prefer this on explicit teardown;
    /// [`Self::disconnect`] is the non-blocking drop used by `Drop` paths
    /// where awaiting is impossible (a stale pooled connection would block
    /// quit on the `BYE` wait — closing the socket reaps server state just
    /// as well, like a network drop).
    pub async fn logout(&mut self) {
        if let Some(session) = self.session.as_mut() {
            let _ =
                tokio::time::timeout(COMMAND_TIMEOUT, session.execute(CommandBody::Logout)).await;
        }
        self.session = None;
    }

    pub fn disconnect(&mut self) {
        self.session = None;
    }

    pub async fn reconnect(&mut self, password: Option<&str>) -> Result<()> {
        self.session = None;
        if let Some(pw) = password {
            return self.connect(pw).await;
        }
        let secrets = crate::auth::load_account_secrets(&self.account.auth_vault_key)
            .map_err(|e| StoreError::NotFound(format!("keyring secret: {e}")))?;
        self.connect(&secrets.imap_password).await
    }

    /// Liveness probe for pooled sessions: one NOOP round-trip. `false` =
    /// dead, half-closed, or never connected — the caller should drop this
    /// session and connect fresh rather than send real work into it.
    pub async fn is_healthy(&mut self) -> bool {
        match self.session.as_mut() {
            Some(s) => s.noop().await.is_ok(),
            None => false,
        }
    }

    /// Cheap synchronous presence check for `Drop`/`checkin` paths that
    /// cannot await a NOOP round-trip. Staleness is detected at checkout
    /// via [`Self::is_healthy`].
    pub fn is_connected(&self) -> bool {
        self.session.is_some()
    }

    pub async fn capabilities_list(&mut self) -> Result<Vec<String>> {
        let session = self.session()?;
        session.capability().await
    }
}
