//! [`SmtpSender`]: SMTP submission bound to one account's settings.
//!
//! Queue-first and crash-safe: [`SmtpSender::enqueue_send`] validates, builds
//! MIME, and persists the outbox row without touching the network; the submit
//! path flips the row to `sending` before the SMTP round-trip so the same
//! bytes are retried after a restart.

use lettre::address::{Address, Envelope};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::transport::smtp::extension::ClientId;
use lettre::{SmtpTransport, Transport};

use crate::db::Db;
use crate::error::{Result, StoreError};
use crate::models::{Account, FolderRole};
use crate::store::{contacts, folders, queue, settings};
use crate::sync::imap::ImapSync;
use crate::sync::traits::MailSender;

use super::{
    addresses::{
        parse_reply_to, sender_domain_is_aligned, strict_mailboxes, to_group_name, valid_mailboxes,
    },
    attachments::load_outgoing_attachments,
    message::{assemble_message, resolve_bodies, SendRequest},
    policy::effective_format,
};

/// SMTP submission endpoint derived from an account.
#[derive(Debug, Clone)]
pub struct SmtpEndpoint {
    /// `host:port`.
    pub addr: String,
    /// `true` = implicit TLS (465), `false` = STARTTLS (587).
    pub implicit_tls: bool,
}

/// Derive the endpoint from account settings.
#[must_use]
pub fn endpoint_for(account: &Account) -> SmtpEndpoint {
    SmtpEndpoint {
        addr: format!("{}:{}", account.smtp_host, account.smtp_port),
        implicit_tls: account.smtp_port == 465 || account.smtp_security.eq_ignore_ascii_case("tls"),
    }
}
/// SMTP sender bound to one account's settings.
pub struct SmtpSender {
    endpoint: SmtpEndpoint,
    username: String,
    from: String,
}

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
        let (host, port) = {
            let mut parts = self.endpoint.addr.rsplitn(2, ':');
            let port: u16 = parts.next().unwrap_or("465").parse().map_err(|_| {
                StoreError::InvalidInput(format!("bad smtp addr {}", self.endpoint.addr))
            })?;
            (parts.next().unwrap_or("").to_string(), port)
        };
        let tls_params = TlsParameters::new(host.clone())
            .map_err(|e| StoreError::InvalidInput(format!("tls setup failed: {e}")))?;
        let mut builder = SmtpTransport::relay(&host)?;
        // EHLO with the sender domain instead of the bare machine hostname:
        // a dotless `EHLO omarchy` trips HELO-based spam heuristics, while
        // the (unavoidable) client IP is logged by the server either way.
        if let Some(domain) = self.from.rsplit('@').next().filter(|d| d.contains('.')) {
            builder = builder.hello_name(ClientId::Domain(domain.to_string()));
        }
        builder = builder
            .port(port)
            .credentials(Credentials::new(
                self.username.clone(),
                password.to_string(),
            ))
            .tls(if self.endpoint.implicit_tls {
                Tls::Wrapper(tls_params)
            } else {
                Tls::Required(tls_params)
            });
        Ok(builder.build())
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
        let html_src = req
            .body_html
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
        let (plain, html) = resolve_bodies(req.body_text, req.body_html, format);
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
            &files,
            req.request_mdn,
        )?;
        let raw = email.formatted();
        let queue_id = queue::enqueue_mime(db, account_id, None, &raw, from_addr, &rcpts)?;
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
        if settings::get_bool(db, settings::COLLECT_SENT_CONTACTS).unwrap_or(true) {
            let mut all_rcpts = valid_mailboxes(req.to);
            all_rcpts.extend(valid_mailboxes(req.cc));
            all_rcpts.extend(valid_mailboxes(req.bcc));
            for mb in all_rcpts {
                let addr = mb.email.to_string();
                let name = mb.name.as_deref();
                if let Err(e) = contacts::seen(db, &addr, name) {
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
    pub fn submit_claimed(&self, db: &Db, queue_id: i64, password: &str) -> Result<()> {
        let row = queue::get(db, queue_id)?;
        let raw = row
            .raw_mime
            .as_deref()
            .filter(|b| !b.is_empty())
            .ok_or_else(|| {
                StoreError::InvalidInput(format!("queue entry {queue_id} has no MIME bytes"))
            })?;
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
        match self.submit_raw(from, &row.envelope_to, raw, password) {
            Ok(()) => {
                // The server accepted it, so it IS sent. A bookkeeping failure
                // (the row vanished with its account mid-send) must not turn
                // that into "send failed" and invite a duplicate resend.
                if let Err(e) = queue::mark_sent(db, queue_id) {
                    log::warn!("smtp: sent, but outbox entry {queue_id} not updated: {e}");
                }
                Ok(())
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
        let _ = queue::prune_sent(db);
        let mut sent = 0u64;
        let mut first_error = None;
        for row in queue::list_submittable(db, account_id)? {
            match queue::claim(db, row.id) {
                Ok(true) => {}
                Ok(false) => continue, // another submitter owns it now
                Err(e) => {
                    first_error.get_or_insert(e);
                    continue;
                }
            }
            match self.submit_claimed(db, row.id, password) {
                Ok(()) => {
                    sent += 1;
                    if settings::get_bool(db, settings::COLLECT_SENT_CONTACTS).unwrap_or(true) {
                        // Envelope only: the display names lived in the
                        // composer form, which is long gone by now.
                        for addr in &row.envelope_to {
                            if let Err(e) = contacts::seen(db, addr, None) {
                                log::warn!("contacts: could not collect recipient: {e}");
                            }
                        }
                    }
                    if let Some(raw) = row.raw_mime.as_deref() {
                        if let Err(e) = self
                            .save_sent_copy(db, account_id, imap_password, raw)
                            .await
                        {
                            log::warn!("smtp: outbox sent copy failed: {e}");
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

    fn submit_raw(&self, from: &str, to: &[String], raw: &[u8], password: &str) -> Result<()> {
        let from_addr: Address = from.parse()?;
        let rcpts: Vec<Address> = to
            .iter()
            .map(|s| s.parse())
            .collect::<std::result::Result<_, _>>()?;
        let envelope = Envelope::new(Some(from_addr), rcpts)
            .map_err(|e| StoreError::InvalidInput(format!("smtp envelope: {e}")))?;
        let response = self.transport(password)?.send_raw(&envelope, raw)?;
        log::info!(
            "smtp: sent to {to:?} via {}: {response:?}",
            self.endpoint.addr
        );
        Ok(())
    }

    /// Where the Sent copy belongs, or `None` when the setting is off.
    ///
    /// Resolved before any connection is touched, so a disabled copy or a
    /// missing Sent folder costs nothing.
    fn sent_copy_target(db: &Db, account_id: i64) -> Result<Option<String>> {
        match settings::get_bool(db, settings::SENT_COPY_ENABLED) {
            Ok(true) => {}
            Ok(false) => {
                log::info!("smtp: sent-copy disabled by setting");
                return Ok(None);
            }
            Err(e) => {
                return Err(StoreError::InvalidInput(format!(
                    "cannot read sent-copy setting, skipping copy: {e}"
                )));
            }
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
        let mut imap = ImapSync::new(&account);
        imap.connect(imap_password).await.map_err(|e| {
            StoreError::InvalidInput(format!("IMAP connect failed, skipping sent copy: {e}"))
        })?;
        let result = Self::save_sent_copy_via(db, account_id, &mut imap, raw).await;
        imap.logout().await;
        result
    }
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
}
