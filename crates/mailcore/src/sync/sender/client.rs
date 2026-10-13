//! [`SmtpSender`]: SMTP submission bound to one account's settings.
//!
//! Queue-first and crash-safe: [`SmtpSender::enqueue_send`] validates, builds
//! MIME, and persists the outbox row without touching the network; the submit
//! path flips the row to `sending` before the SMTP round-trip so the same
//! bytes are retried after a restart.

use std::time::Duration;

use lettre::address::{Address, Envelope};
use lettre::message::{Mailbox, Message};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{SmtpTransport, Transport};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Account, FolderRole, QueuedSend};
use crate::store::{account_settings, accounts, contacts, folders, queue, settings};
use crate::sync::imap::ImapSync;
use crate::sync::traits::MailSender;

use super::{
    addresses::{
        parse_reply_to, sender_domain_is_aligned, strict_mailboxes, to_group_name, valid_mailboxes,
    },
    attachments::load_outgoing_attachments,
    dsn::{send_with_dsn, Connect},
    message::{assemble_message, resolve_bodies, split_inline_images, SendRequest},
    policy::effective_format,
};

/// SMTP submission endpoint derived from an account.
#[derive(Debug, Clone)]
pub struct SmtpEndpoint {
    /// `host:port`.
    pub addr: String,
    /// `true` = implicit TLS (465), `false` = STARTTLS (587).
    pub implicit_tls: bool,
    /// Security `none`: the account's explicit opt-in to no encryption.
    pub plaintext: bool,
}

/// Derive the endpoint from account settings.
#[must_use]
pub fn endpoint_for(account: &Account) -> SmtpEndpoint {
    SmtpEndpoint {
        addr: format!("{}:{}", account.smtp_host, account.smtp_port),
        implicit_tls: account.smtp_port == 465 || account.smtp_security.eq_ignore_ascii_case("tls"),
        plaintext: crate::store::account_form::is_plaintext(&account.smtp_security),
    }
}
/// What a successful submit learned on the way.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Submitted {
    /// A delivery confirmation was asked for, but the server does not
    /// offer DSN: the mail went without it and no confirmation will come.
    pub dsn_unsupported: bool,
    /// The bytes as submitted: for a scheduled send the `Date:` header is
    /// refreshed to the send time, so the Sent copy files the same bytes.
    pub raw: Vec<u8>,
}

/// SMTP sender bound to one account's settings.
pub struct SmtpSender {
    endpoint: SmtpEndpoint,
    username: String,
    from: String,
}

/// Socket timeout for a submit, and for the delivery-report connection.
///
/// lettre's `None` is not "the default": it disables the connect timeout *and*
/// the read/write timeouts, so a half-open or silently black-holing SMTP host
/// blocked the one `mailclient-net` thread forever — and with it every queued
/// IMAP job, sync and download. The setup form keeps its own shorter
/// `test_connection` timeout so a typo in the host fails the test quickly
/// rather than after this long.
pub(super) const SUBMIT_TIMEOUT: Duration = Duration::from_secs(30);

impl SmtpSender {
    /// Build from account settings (password supplied per-send).
    #[must_use]
    pub fn new(account: &Account) -> Self {
        Self {
            endpoint: endpoint_for(account),
            username: account.smtp_username.clone(),
            from: account.email_address.clone(),
        }
    }

    fn transport(&self, password: &str) -> Result<SmtpTransport> {
        self.transport_with_timeout(password, Some(SUBMIT_TIMEOUT))
    }

    /// Login check without sending: connect, EHLO, TLS, AUTH, NOOP. Used by
    /// the setup-form connection test with its own short timeout, so a dead
    /// server fails the test instead of stalling it (sends use the longer
    /// [`SUBMIT_TIMEOUT`]).
    pub fn test_connection(&self, password: &str) -> Result<()> {
        let transport = self.transport_with_timeout(password, Some(Duration::from_secs(15)))?;
        match transport.test_connection() {
            Ok(true) => Ok(()),
            Ok(false) => Err(StoreError::InvalidInput("smtp: not connected".to_string())),
            Err(e) => Err(StoreError::InvalidInput(format!("smtp: {e}"))),
        }
    }

    fn transport_with_timeout(
        &self,
        password: &str,
        timeout: Option<Duration>,
    ) -> Result<SmtpTransport> {
        let c = self.connect_params(password)?;
        Ok(SmtpTransport::builder_dangerous(c.host)
            .port(c.port)
            .hello_name(c.hello)
            .credentials(c.credentials)
            .timeout(timeout)
            .tls(c.tls)
            .build())
    }

    /// Host, port, EHLO name, TLS mode and login for this account: the
    /// pooled transport and the DSN connection are built from the same.
    fn connect_params(&self, password: &str) -> Result<Connect> {
        let (host, port) = {
            let mut parts = self.endpoint.addr.rsplitn(2, ':');
            let port: u16 = parts.next().unwrap_or("465").parse().map_err(|_| {
                StoreError::InvalidInput(format!("bad smtp addr {}", self.endpoint.addr))
            })?;
            (parts.next().unwrap_or("").to_string(), port)
        };
        let tls_params = TlsParameters::new(host.clone())
            .map_err(|e| StoreError::InvalidInput(format!("tls setup failed: {e}")))?;
        // EHLO with the sender domain instead of the bare machine hostname:
        // a dotless `EHLO omarchy` trips HELO-based spam heuristics, while
        // the (unavoidable) client IP is logged by the server either way.
        let hello = match self.from.rsplit('@').next().filter(|d| d.contains('.')) {
            Some(domain) => ClientId::Domain(domain.to_string()),
            None => ClientId::default(),
        };
        Ok(Connect {
            host,
            port,
            hello,
            tls: if self.endpoint.plaintext {
                Tls::None
            } else if self.endpoint.implicit_tls {
                Tls::Wrapper(tls_params)
            } else {
                Tls::Required(tls_params)
            },
            credentials: Credentials::new(self.username.clone(), password.to_string()),
        })
    }
}
impl MailSender for SmtpSender {
    async fn send_raw(&mut self, db: &Db, account_id: i64, req: &SendRequest<'_>) -> Result<()> {
        let raw = self.submit(db, account_id, req)?;
        // Never fails the send itself (kept for the harness path).
        if let Err(e) = self
            .save_sent_copy(db, account_id, req.imap_password, &raw)
            .await
        {
            log::warn!("smtp: sent copy failed (send itself succeeded): {e}");
        }
        Ok(())
    }
}
impl SmtpSender {
    /// Validate, build and enqueue one message without touching the network:
    /// address checks, MIME assembly (incl. attachment reads) and the outbox
    /// row. Pure local work, so the composer can run it synchronously for
    /// instant feedback and close before any network happens. Returns the
    /// outbox row id plus the raw MIME (for the Sent copy). `password` /
    /// `imap_password` are unused here — they only matter at submit time.
    pub fn enqueue_send(
        &self,
        db: &Db,
        account_id: i64,
        req: &SendRequest<'_>,
    ) -> Result<(i64, Vec<u8>)> {
        self.enqueue_send_at(db, account_id, req, None)
    }

    /// [`Self::enqueue_send`] for a future instant (`Some` UTC RFC3339): the
    /// row waits as `scheduled` until its time comes instead of submitting
    /// with the next flush. `None` is the immediate path.
    pub fn enqueue_send_at(
        &self,
        db: &Db,
        account_id: i64,
        req: &SendRequest<'_>,
        send_at: Option<&str>,
    ) -> Result<(i64, Vec<u8>)> {
        let to_boxes = valid_mailboxes(req.to);
        let cc_boxes = strict_mailboxes("Cc", req.cc)?;
        let bcc_boxes = strict_mailboxes("Bcc", req.bcc)?;
        let mut rcpts: Vec<String> = to_boxes.iter().map(|m| m.email.to_string()).collect();
        rcpts.extend(cc_boxes.iter().map(|m| m.email.to_string()));
        rcpts.extend(bcc_boxes.iter().map(|m| m.email.to_string()));
        if rcpts.is_empty() {
            return Err(StoreError::InvalidInput(
                "add at least one recipient (To, Cc or Bcc)".to_string(),
            ));
        }
        let rcpt_refs: Vec<&str> = rcpts.iter().map(String::as_str).collect();
        req.policy.check(&rcpt_refs)?;

        let from_addr: &str = req.from.filter(|s| !s.is_empty()).unwrap_or(&self.from);
        if !from_addr.contains('@') {
            return Err(StoreError::InvalidInput(format!(
                "invalid sender address: {from_addr}"
            )));
        }
        if !self.from.contains('@') {
            return Err(StoreError::InvalidInput(
                "account email address has no domain".to_string(),
            ));
        }
        // SPF, DKIM, and DMARC must align with the visible From domain. SMTP
        // providers sign after submission, so never allow a caller to bypass
        // the composer's same-domain sender restriction.
        if !sender_domain_is_aligned(from_addr, &self.from) {
            return Err(StoreError::InvalidInput(
                "sender domain must match the account domain to preserve SPF/DKIM/DMARC alignment"
                    .to_string(),
            ));
        }
        // Display name from the composer, else the account default — empty
        // means address-only `From:`.
        let from_name = req.from_name.map(str::trim).filter(|s| !s.is_empty());
        let from_box = match from_name {
            Some(name) => lettre::message::Mailbox::new(Some(name.to_string()), from_addr.parse()?),
            None => from_addr.parse()?,
        };
        // Auto resolves per message: formatting present → HTML (with a plain
        // twin when enabled), otherwise plain text.
        let (body_html, inlines) = split_inline_images(req.body_html, from_addr)?;
        let html_src = body_html
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                if crate::html::looks_like_html(req.body_text) {
                    Some(req.body_text)
                } else {
                    None
                }
            });
        let needs_html = html_src
            .map(|h| crate::html::needs_html_formatting(&crate::html::sanitize_for_send(h)))
            .unwrap_or(false);
        let format = effective_format(req.format, needs_html, req.include_plain);
        let (plain, html) = resolve_bodies(req.body_text, body_html.as_deref(), format);
        let files = load_outgoing_attachments(req.attachments)?;
        let to_group = to_boxes
            .is_empty()
            .then(|| to_group_name(&req.to.join(" ")));
        let reply_to = parse_reply_to(req.reply_to.unwrap_or(""))?;
        // Normalized mailbox strings (display names preserved): headers and
        // envelope agree, and malformed strings never reach the SMTP envelope.
        let cc: Vec<String> = cc_boxes.iter().map(|m| m.to_string()).collect();
        let bcc: Vec<String> = bcc_boxes.iter().map(|m| m.to_string()).collect();
        let email = assemble_message(
            from_box,
            req.subject,
            to_boxes,
            to_group.as_deref(),
            &cc,
            &bcc,
            reply_to,
            format,
            plain,
            html,
            &inlines,
            &files,
            req.request_mdn,
        )?;
        let raw = email.formatted();
        let queue_id = match send_at {
            Some(at) => queue::enqueue_scheduled_mime(
                db,
                account_id,
                &queue::ScheduledMime {
                    message_id: None,
                    raw_mime: &raw,
                    envelope_from: from_addr,
                    envelope_to: &rcpts,
                    request_dsn: req.request_dsn,
                    notify_self: req.notify_self,
                    send_at: at,
                },
            )?,
            None => queue::enqueue_mime(
                db,
                account_id,
                None,
                &raw,
                from_addr,
                &rcpts,
                req.request_dsn,
            )?,
        };
        Ok((queue_id, raw))
    }

    /// SMTP-submit one message: everything up to the server accepting it.
    /// Returns the raw MIME for the Sent copy. Enqueues first (crash-safe),
    /// then submits the row; on failure the row's bytes are discarded so a
    /// manual retry cannot deliver the same message twice.
    pub fn submit(&mut self, db: &Db, account_id: i64, req: &SendRequest<'_>) -> Result<Vec<u8>> {
        let (queue_id, raw) = self.enqueue_send(db, account_id, req)?;
        if let Err(e) = self.submit_claimed(db, queue_id, req.password) {
            // The user is about to see this failure and owns the retry. Leaving
            // submittable bytes behind would let the next sync deliver the same
            // message again, duplicating whatever they resend by hand.
            let _ = queue::discard_mime(db, queue_id);
            return Err(e);
        }
        if account_settings::get_bool(db, account_id, settings::COLLECT_SENT_CONTACTS) {
            let mut all_rcpts = valid_mailboxes(req.to);
            all_rcpts.extend(valid_mailboxes(req.cc));
            all_rcpts.extend(valid_mailboxes(req.bcc));
            for mb in all_rcpts {
                let addr = mb.email.to_string();
                let name = mb.name.as_deref();
                if let Err(e) = contacts::seen_sent(db, &addr, name) {
                    log::warn!("contacts: could not collect recipient: {e}");
                }
            }
        }
        Ok(raw)
    }
}
impl SmtpSender {
    /// Submit one outbox row the caller already owns: one it just created with
    /// [`Self::enqueue_send`] (rows are born claimed) or won via
    /// [`queue::claim`]. Crash-safe: MIME is already on disk and the row stays
    /// `sending` through the SMTP round-trip, so a crash leaves it for
    /// [`queue::requeue_interrupted`] to retry with the same bytes.
    pub fn submit_claimed(&self, db: &Db, queue_id: i64, password: &str) -> Result<Submitted> {
        self.submit_claimed_via(db, queue_id, password, &mut None)
    }

    /// [`Self::submit_claimed`] over `transport`, built on first use and then
    /// kept by the caller. Every built transport starts lettre's pool thread,
    /// which lives for its idle timeout, so a flush that built one per row
    /// left one thread per message behind (B5).
    fn submit_claimed_via(
        &self,
        db: &Db,
        queue_id: i64,
        password: &str,
        transport: &mut Option<SmtpTransport>,
    ) -> Result<Submitted> {
        let row = queue::get(db, queue_id)?;
        let mut raw = row
            .raw_mime
            .clone()
            .filter(|b| !b.is_empty())
            .ok_or_else(|| {
                StoreError::InvalidInput(format!("queue entry {queue_id} has no MIME bytes"))
            })?;
        // A scheduled send's MIME was built when it was composed, days ago:
        // refresh `Date:` to the actual send time so the recipient's list
        // sorts it under today, not under composition day.
        if row.send_at.is_some() {
            raw = Self::refresh_date_header(&raw);
            if let Err(e) = queue::update_raw_mime(db, queue_id, &raw) {
                log::warn!("smtp: outbox entry {queue_id} date not refreshed: {e}");
            }
        }
        let from = row
            .envelope_from
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| StoreError::InvalidInput("queued send missing envelope from".into()))?;
        if row.envelope_to.is_empty() {
            return Err(StoreError::InvalidInput(
                "queued send has no envelope recipients".into(),
            ));
        }
        match self.submit_raw(
            from,
            &row.envelope_to,
            &raw,
            password,
            row.request_dsn,
            transport,
        ) {
            Ok(mut submitted) => {
                // The server accepted it, so it IS sent. A bookkeeping failure
                // (the row vanished with its account mid-send) must not turn
                // that into "send failed" and invite a duplicate resend.
                if let Err(e) = queue::mark_sent(db, queue_id) {
                    log::warn!("smtp: sent, but outbox entry {queue_id} not updated: {e}");
                }
                submitted.raw = raw;
                Ok(submitted)
            }
            Err(e) => {
                let _ = queue::mark_failed(db, queue_id, &e.to_string());
                Err(e)
            }
        }
    }

    /// Retry every submittable outbox row for this account — rows left in
    /// `sending` by a crash, or earlier flushes that failed transiently. A row
    /// whose failure already reached the user has no MIME left and is skipped.
    /// Every row is claimed atomically first, so a GUI and a `--sync-once` run
    /// flushing the same DB at once never both submit it.
    ///
    /// Each row is delivered exactly as the original send would have been,
    /// Sent copy included. One row failing does not stop the rest: a permanent
    /// rejection would otherwise block every message queued behind it until
    /// its retry budget ran out. Errors are logged per row; one is returned
    /// only when nothing at all got through, so a partial flush still reports
    /// what it delivered.
    pub async fn flush_outbox(
        &self,
        db: &Db,
        account_id: i64,
        password: &str,
        imap_password: Option<&str>,
    ) -> Result<u64> {
        let _ = queue::requeue_interrupted(db, account_id);
        // Scheduled sends whose time has come join this flush.
        let _ = queue::promote_due(db, account_id);
        let _ = queue::prune_sent(db);
        let mut sent = 0u64;
        let mut first_error = None;
        let mut transport = None;
        for row in queue::list_submittable(db, account_id)? {
            match queue::claim(db, row.id) {
                Ok(true) => {}
                Ok(false) => continue, // another submitter owns it now
                Err(e) => {
                    first_error.get_or_insert(e);
                    continue;
                }
            }
            match self.submit_claimed_via(db, row.id, password, &mut transport) {
                Ok(submitted) => {
                    sent += 1;
                    if account_settings::get_bool(db, account_id, settings::COLLECT_SENT_CONTACTS) {
                        // Envelope only: the display names lived in the
                        // composer form, which is long gone by now.
                        for addr in &row.envelope_to {
                            if let Err(e) = contacts::seen_sent(db, addr, None) {
                                log::warn!("contacts: could not collect recipient: {e}");
                            }
                        }
                    }
                    if !submitted.raw.is_empty() {
                        if let Err(e) = self
                            .save_sent_copy(db, account_id, imap_password, &submitted.raw)
                            .await
                        {
                            log::warn!("smtp: outbox sent copy failed: {e}");
                        }
                    }
                    if row.notify_self {
                        // The user asked for a receipt to the own address.
                        // The mail itself is already out, so a failed
                        // receipt only logs itself and never fails the flush.
                        if let Err(e) = self
                            .send_self_receipt(
                                db,
                                account_id,
                                &row,
                                password,
                                imap_password,
                                &mut transport,
                            )
                            .await
                        {
                            log::warn!("smtp: outbox entry {} self receipt failed: {e}", row.id);
                        }
                    }
                }
                Err(e) => {
                    log::warn!("smtp: outbox entry {} failed: {e}", row.id);
                    first_error.get_or_insert(e);
                }
            }
        }
        match first_error {
            Some(e) if sent == 0 => Err(e),
            _ => Ok(sent),
        }
    }

    /// Mail a delivery receipt for a scheduled send to the account's own
    /// address: what went out, to whom, and when. The receipt is an ordinary
    /// immediate row (no `notify_self` of its own, so it cannot chain) sent
    /// over the caller's transport, with a Sent copy like any other send.
    async fn send_self_receipt(
        &self,
        db: &Db,
        account_id: i64,
        orig: &QueuedSend,
        password: &str,
        imap_password: Option<&str>,
        transport: &mut Option<SmtpTransport>,
    ) -> Result<()> {
        let raw = self_receipt_mime(&self.from, orig)?;
        let to = vec![self.from.clone()];
        let id = queue::enqueue_mime(db, account_id, None, &raw, &self.from, &to, false)?;
        let submitted = self.submit_claimed_via(db, id, password, transport)?;
        if !submitted.raw.is_empty() {
            self.save_sent_copy(db, account_id, imap_password, &submitted.raw)
                .await?;
        }
        log::info!("smtp: scheduled-send receipt mailed to self");
        Ok(())
    }

    /// Rewrite a stored MIME message's `Date:` header to now (RFC 5322), for a
    /// scheduled send whose bytes were built at composition time: without this
    /// the recipient's list would sort it under composition day. Byte surgery on
    /// the header block only (before the blank line) — bodies, which may hold
    /// non-UTF-8 attachment bytes, are never decoded. A folded `Date:` loses its
    /// continuations; a missing one is prepended. Never fails the send:
    /// anything unexpected goes out with its original bytes.
    fn refresh_date_header(raw: &[u8]) -> Vec<u8> {
        let now = chrono::Utc::now()
            .format("%a, %d %b %Y %H:%M:%S %z")
            .to_string();
        let crlf = raw.windows(2).any(|w| w == b"\r\n");
        let nl: &[u8] = if crlf { b"\r\n" } else { b"\n" };
        let head_end = Self::find_slice(raw, b"\r\n\r\n")
            .map(|i| i + 4)
            .or_else(|| Self::find_slice(raw, b"\n\n").map(|i| i + 2))
            .unwrap_or(raw.len());
        let (head, body) = (raw[..head_end].to_vec(), &raw[head_end..]);
        let is_date_line = |line: &[u8]| {
            line.len() > 5 && line[4] == b':' && line[..4].eq_ignore_ascii_case(b"date")
        };
        let mut out: Vec<u8> = Vec::with_capacity(raw.len() + 64);
        let mut replaced = false;
        let mut skipping_fold = false;
        // Split keeping endings: iterate on `\n`, re-emit the `\r` when present.
        let mut start = 0;
        for (i, b) in head.iter().enumerate() {
            if *b != b'\n' {
                continue;
            }
            let mut line = &head[start..i];
            let had_cr = line.ends_with(b"\r");
            if had_cr {
                line = &line[..line.len() - 1];
            }
            let ending: &[u8] = if had_cr { b"\r\n" } else { b"\n" };
            if skipping_fold {
                if line.starts_with(b" ") || line.starts_with(b"\t") {
                    start = i + 1;
                    continue;
                }
                skipping_fold = false;
            }
            if !replaced && is_date_line(line) {
                out.extend_from_slice(format!("Date: {now}").as_bytes());
                out.extend_from_slice(ending);
                replaced = true;
                skipping_fold = true;
            } else {
                out.extend_from_slice(line);
                out.extend_from_slice(ending);
            }
            start = i + 1;
        }
        // A header block without trailing newline (or no blank line at all):
        // handle the tail the same way.
        if start < head.len() {
            let line = &head[start..];
            if skipping_fold && (line.starts_with(b" ") || line.starts_with(b"\t")) {
                // Folded tail of the replaced Date: dropped.
            } else if !replaced && is_date_line(line) {
                out.extend_from_slice(format!("Date: {now}").as_bytes());
                replaced = true;
            } else {
                out.extend_from_slice(line);
            }
        }
        if !replaced {
            let mut with_date = format!("Date: {now}").into_bytes();
            with_date.extend_from_slice(nl);
            with_date.extend_from_slice(&out);
            out = with_date;
        }
        out.extend_from_slice(body);
        out
    }

    /// First index of `needle` in `haystack`, if present.
    fn find_slice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }
    fn submit_raw(
        &self,
        from: &str,
        to: &[String],
        raw: &[u8],
        password: &str,
        request_dsn: bool,
        transport: &mut Option<SmtpTransport>,
    ) -> Result<Submitted> {
        let from_addr: Address = from.parse()?;
        let rcpts: Vec<Address> = to
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let envelope = Envelope::new(Some(from_addr), rcpts)
            .map_err(|e| StoreError::InvalidInput(format!("smtp envelope: {e}")))?;
        if request_dsn {
            let dsn = send_with_dsn(&self.connect_params(password)?, &envelope, raw)?;
            // A count, not the addresses: recipients are mail data (AGENTS §6).
            log::info!(
                "smtp: sent to {} recipient(s) via {} (delivery confirmation: {dsn})",
                to.len(),
                self.endpoint.addr
            );
            return Ok(Submitted {
                dsn_unsupported: !dsn,
                raw: raw.to_vec(),
            });
        }
        let transport = match transport {
            Some(t) => t,
            None => transport.insert(self.transport(password)?),
        };
        let response = transport.send_raw(&envelope, raw)?;
        log::info!(
            "smtp: sent to {} recipient(s) via {}: {}",
            to.len(),
            self.endpoint.addr,
            response.code()
        );
        Ok(Submitted {
            dsn_unsupported: false,
            raw: raw.to_vec(),
        })
    }

    /// Where the Sent copy belongs, or `None` when the setting is off.
    ///
    /// Resolved before any connection is touched, so a disabled copy or a
    /// missing Sent folder costs nothing.
    fn sent_copy_target(db: &Db, account_id: i64) -> Result<Option<String>> {
        if !account_settings::get_bool(db, account_id, settings::SENT_COPY_ENABLED) {
            log::info!("smtp: sent-copy disabled by setting");
            return Ok(None);
        }
        folders::list_by_account(db, account_id)
            .map_err(|e| {
                StoreError::InvalidInput(format!("cannot list folders, skipping sent copy: {e}"))
            })?
            .into_iter()
            .find(|f| f.role == FolderRole::Sent)
            .map(|f| Some(f.path))
            .ok_or_else(|| {
                StoreError::InvalidInput("no Sent folder known, skipping sent copy".to_string())
            })
    }

    /// Gmail files every accepted SMTP submission into Sent Mail itself.
    /// APPENDing our own copy afterwards delivers once but files twice —
    /// the "double in Sent" every Gmail user reports. Host-based on
    /// purpose: it describes one provider's server behaviour, not the
    /// user's mail content.
    fn gmail_autofiles_sent(account: &Account) -> bool {
        [&account.imap_host, &account.smtp_host].iter().any(|h| {
            let lower = h.to_ascii_lowercase();
            lower.contains("gmail") || lower.contains("googlemail")
        })
    }

    /// File the sent MIME bytes into the account's Sent folder over a session
    /// the caller already holds.
    ///
    /// Preferred wherever a session is available: [`Self::save_sent_copy`]
    /// dials a second TCP + TLS + LOGIN of its own, which the user waits
    /// through after the mail is already gone.
    ///
    /// A disabled setting skips silently (`Ok` — intentional, not a failure);
    /// every genuine failure is returned so the caller can say the Sent copy
    /// is missing rather than leave it looking sent-but-unsaved.
    pub async fn save_sent_copy_via(
        db: &Db,
        account_id: i64,
        imap: &mut ImapSync,
        raw: &[u8],
    ) -> Result<()> {
        let account = accounts::get(db, account_id).map_err(|e| {
            StoreError::InvalidInput(format!("cannot load account, skipping sent copy: {e}"))
        })?;
        if Self::gmail_autofiles_sent(&account) {
            log::info!("smtp: Gmail files submissions itself, skipping APPEND copy");
            return Ok(());
        }
        let Some(sent_path) = Self::sent_copy_target(db, account_id)? else {
            return Ok(());
        };
        imap.append_to_folder(&sent_path, raw)
            .await
            .map_err(|e| StoreError::InvalidInput(format!("APPEND to {sent_path} failed: {e}")))?;
        log::info!("smtp: saved copy to {sent_path}");
        Ok(())
    }

    /// File the sent MIME bytes into the account's Sent folder, connecting a
    /// session for it.
    ///
    /// For callers with no session to lend — the outbox flush and the
    /// headless CLI, which run in processes that keep no pool. Inside the
    /// app, prefer [`Self::save_sent_copy_via`].
    pub async fn save_sent_copy(
        &self,
        db: &Db,
        account_id: i64,
        imap_password: Option<&str>,
        raw: &[u8],
    ) -> Result<()> {
        if Self::sent_copy_target(db, account_id)?.is_none() {
            return Ok(());
        }
        let Some(imap_password) = imap_password else {
            return Err(StoreError::InvalidInput(
                "no IMAP credential, skipping sent copy".to_string(),
            ));
        };
        let account = crate::store::accounts::get(db, account_id).map_err(|e| {
            StoreError::InvalidInput(format!("cannot load account, skipping sent copy: {e}"))
        })?;
        if Self::gmail_autofiles_sent(&account) {
            log::info!("smtp: Gmail files submissions itself, skipping APPEND copy");
            return Ok(());
        }
        let mut imap = ImapSync::new(&account);
        imap.connect(imap_password).await.map_err(|e| {
            StoreError::InvalidInput(format!("IMAP connect failed, skipping sent copy: {e}"))
        })?;
        let result = Self::save_sent_copy_via(db, account_id, &mut imap, raw).await;
        imap.logout().await;
        result
    }
}

/// Build the receipt for one delivered scheduled send, addressed to
/// `self_addr` (the account's own address).
///
/// Names the original subject, when it went out and how many recipients it
/// had — never the recipients themselves: the receipt is written to the
/// outbox and filed into Sent, and no recipient list belongs in either.
fn self_receipt_mime(self_addr: &str, orig: &QueuedSend) -> Result<Vec<u8>> {
    let raw = orig.raw_mime.as_deref().unwrap_or(&[]);
    let subject =
        crate::outbox::subject_from_mime(raw).unwrap_or_else(|| "(no subject)".to_string());
    // The row's own `send_at` is kept through promotion, so this is when the
    // mail was due; the flush running now is the actual delivery.
    let body = format!(
        "Scheduled send delivered.\n\nSubject: {subject}\nSent: {}\nRecipients: {}\n",
        crate::store::now(),
        orig.envelope_to.len(),
    );
    let email = Message::builder()
        .from(self_addr.parse::<Mailbox>()?)
        .to(self_addr.parse::<Mailbox>()?)
        .subject(format!("Scheduled send delivered: {subject}"))
        .body(body)?;
    Ok(email.formatted().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::db::Db;

    use super::super::policy::{SendFormat, SendPolicy};
    use super::super::support::test_account;

    #[test]
    fn endpoint_prefers_implicit_tls_on_465() {
        let ep = endpoint_for(&test_account());
        assert_eq!(ep.addr, "smtp.x:587");
        assert!(!ep.implicit_tls);
    }

    /// A minimal delivered row for the receipt tests.
    fn held_send() -> QueuedSend {
        QueuedSend {
            id: 1,
            account_id: 1,
            message_id: None,
            status: crate::models::QueueStatus::Queued,
            last_error: None,
            retries: 0,
            raw_mime: None,
            envelope_from: None,
            envelope_to: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
            request_dsn: false,
            send_at: None,
            notify_self: true,
        }
    }

    #[test]
    fn self_receipt_names_subject_time_and_count_but_no_recipients() {
        let raw = b"From: me@example.com\r\nTo: someone@example.com\r\nCc: other@example.com\r\n\
                    Subject: The subject\r\n\r\nbody";
        let orig = QueuedSend {
            raw_mime: Some(raw.to_vec()),
            envelope_from: Some("me@example.com".to_string()),
            envelope_to: vec![
                "someone@example.com".to_string(),
                "other@example.com".to_string(),
            ],
            ..held_send()
        };
        let out = String::from_utf8(self_receipt_mime("me@example.com", &orig).unwrap()).unwrap();
        let (head, body) = out.split_once("\r\n\r\n").expect("header/body split");
        assert!(head.contains("From: me@example.com"), "{head}");
        assert!(head.contains("To: me@example.com"), "{head}");
        assert!(
            head.contains("Subject: Scheduled send delivered: The subject"),
            "{head}"
        );
        assert!(body.contains("Subject: The subject"), "{body}");
        assert!(body.contains("Recipients: 2"), "{body}");
        assert!(body.contains("Sent: 20"), "{body}");
        // Never the recipients: the receipt goes into Sent too.
        assert!(!out.contains("someone@example.com"), "{out}");
        assert!(!out.contains("other@example.com"), "{out}");
    }

    #[test]
    fn self_receipt_of_a_row_without_bytes_still_reads_as_a_receipt() {
        let out =
            String::from_utf8(self_receipt_mime("me@example.com", &held_send()).unwrap()).unwrap();
        assert!(
            out.contains("Subject: Scheduled send delivered: (no subject)"),
            "{out}"
        );
        assert!(out.contains("Recipients: 0"), "{out}");
    }

    #[test]
    fn gmail_autofiles_detection_covers_hosts() {
        let mut a = test_account();
        assert!(!SmtpSender::gmail_autofiles_sent(&a));
        a.smtp_host = "smtp.gmail.com".to_string();
        assert!(SmtpSender::gmail_autofiles_sent(&a));
        a.smtp_host = "smtp.x".to_string();
        a.imap_host = "imap.googlemail.com".to_string();
        assert!(SmtpSender::gmail_autofiles_sent(&a));
    }

    #[test]
    fn enqueue_validates_and_stores_without_network() {
        let db = Db::open_in_memory().unwrap();
        let account = test_account();
        let sender = SmtpSender::new(&account);
        let acc = crate::store::accounts::create(
            &db,
            &crate::models::NewAccount {
                name: "t".to_string(),
                email_address: account.email_address.clone(),
                from_name: String::new(),
                imap_host: "i".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "s".to_string(),
                smtp_port: 587,
                smtp_security: "starttls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 300,
            },
        )
        .unwrap();
        let to = vec!["you@example.com".to_string()];
        let cc = Vec::new();
        let bcc = Vec::new();
        let files = Vec::new();
        let base = SendRequest {
            to: &to,
            cc: &cc,
            bcc: &bcc,
            from: None,
            from_name: None,
            reply_to: None,
            subject: "queued",
            body_text: "hello",
            body_html: None,
            attachments: &files,
            format: SendFormat::Plain,
            include_plain: true,
            policy: &SendPolicy::Unrestricted,
            password: "",
            imap_password: None,
            request_mdn: false,
            request_dsn: false,
            notify_self: false,
        };
        // A bad address fails here — before any network and before a row.
        let bad_cc = vec!["bob@".to_string()];
        let bad = SendRequest {
            cc: &bad_cc,
            ..base
        };
        assert!(sender.enqueue_send(&db, acc, &bad).is_err());
        // A good one stores a submittable row plus the MIME for Sent filing.
        let (id, raw) = sender.enqueue_send(&db, acc, &base).unwrap();
        assert!(id > 0);
        let text = String::from_utf8(raw).unwrap();
        assert!(text.contains("Subject: queued"));
        let row = queue::get(&db, id).unwrap();
        assert!(row.raw_mime.as_deref().is_some_and(|b| !b.is_empty()));
        assert!(!row.envelope_to.is_empty());
    }

    #[test]
    fn scheduled_enqueue_waits_and_date_refresh_keeps_the_body() {
        let db = Db::open_in_memory().unwrap();
        let account = test_account();
        let sender = SmtpSender::new(&account);
        let acc = crate::store::accounts::create(
            &db,
            &crate::models::NewAccount {
                name: "t".to_string(),
                email_address: account.email_address.clone(),
                from_name: String::new(),
                imap_host: "i".to_string(),
                imap_port: 993,
                imap_security: "tls".to_string(),
                imap_username: "u".to_string(),
                smtp_host: "s".to_string(),
                smtp_port: 587,
                smtp_security: "starttls".to_string(),
                smtp_username: "u".to_string(),
                auth_vault_key: "k".to_string(),
                check_interval_secs: 300,
            },
        )
        .unwrap();
        let to = vec!["you@example.com".to_string()];
        let none: Vec<String> = Vec::new();
        let files = Vec::new();
        let req = SendRequest {
            to: &to,
            cc: &none,
            bcc: &none,
            from: None,
            from_name: None,
            reply_to: None,
            subject: "later",
            body_text: "hello",
            body_html: None,
            attachments: &files,
            format: SendFormat::Plain,
            include_plain: true,
            policy: &SendPolicy::Unrestricted,
            password: "",
            imap_password: None,
            request_mdn: false,
            request_dsn: false,
            notify_self: false,
        };
        let at = (chrono::Utc::now() + chrono::Duration::hours(2))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let (id, _) = sender.enqueue_send_at(&db, acc, &req, Some(&at)).unwrap();
        let row = queue::get(&db, id).unwrap();
        assert_eq!(
            row.status,
            crate::models::QueueStatus::Scheduled,
            "scheduled rows wait, they never submit early"
        );
        assert!(queue::list_submittable(&db, acc).unwrap().is_empty());
    }

    #[test]
    fn date_refresh_rewrites_only_the_header() {
        // Binary bodies (attachments) survive byte-identical; only Date moves.
        let mut raw =
            b"From: a@x.y\r\nDate: Mon, 01 Jan 2000 00:00:00 +0000\r\nSubject: hi\r\n\r\n".to_vec();
        raw.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x62, 0x6F, 0x64, 0x79]);
        let out = SmtpSender::refresh_date_header(&raw);
        assert!(
            !out.windows(9).any(|w| w == b"01 Jan 200"),
            "the stale date is gone"
        );
        assert!(out.starts_with(b"From: a@x.y\r\nDate: "));
        assert!(
            out.ends_with(&[0xFF, 0xFE, 0x00, 0x62, 0x6F, 0x64, 0x79]),
            "body bytes are untouched"
        );
        // Exactly one Date header, still CRLF shaped.
        assert_eq!(
            out.windows(5)
                .filter(|w| w.eq_ignore_ascii_case(b"date:"))
                .count(),
            1
        );
        // Missing Date: prepended, blank line kept.
        let nodate = b"Subject: hi\r\n\r\nbody".to_vec();
        let out = SmtpSender::refresh_date_header(&nodate);
        assert!(out.starts_with(b"Date: "));
        assert!(out.windows(4).any(|w| w == b"\r\n\r\n"));
        // A folded Date loses its continuations, nothing else moves.
        let folded = b"Date: Mon, 01 Jan\r\n 2000 +0000\r\nSubject: hi\r\n\r\nx".to_vec();
        let out = SmtpSender::refresh_date_header(&folded);
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("2000 +0000"), "folded tail dropped");
        assert!(text.contains("Subject: hi"));
        assert_eq!(
            text.matches("Date:").count(),
            1,
            "still exactly one Date header"
        );
    }
}
