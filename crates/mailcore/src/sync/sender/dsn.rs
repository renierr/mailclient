//! Delivery confirmations: SMTP DSN (RFC 3461) parameters on `MAIL FROM`
//! and `RCPT TO`.
//!
//! lettre's transport sends no envelope parameters of its own, so a mail
//! that asks for a confirmation goes over lettre's lower-level connection:
//! the same connect, TLS and login steps, plus an EHLO whose reply says
//! whether the server offers DSN at all. A server without it gets the mail
//! plain — the send never fails over a confirmation.

use lettre::address::{Address, Envelope};
use lettre::transport::smtp::authentication::{Credentials, DEFAULT_MECHANISMS};
use lettre::transport::smtp::client::{SmtpConnection, Tls};
use lettre::transport::smtp::commands::{Data, Ehlo, Mail, Rcpt};
use lettre::transport::smtp::extension::{
    ClientId, Extension, MailBodyParameter, MailParameter, RcptParameter,
};
use lettre::transport::smtp::Error;

/// Where and how to connect, shared with the plain transport.
pub(super) struct Connect {
    pub host: String,
    pub port: u16,
    pub hello: ClientId,
    pub tls: Tls,
    pub credentials: Credentials,
}

/// Submit `raw` asking every recipient's server for a delivery report on
/// success, failure and delay. Returns whether the server took the request;
/// `false` means it does not offer DSN and the mail went without it.
pub(super) fn send_with_dsn(
    connect: &Connect,
    envelope: &Envelope,
    raw: &[u8],
) -> Result<bool, Error> {
    let wrapper = match &connect.tls {
        Tls::Wrapper(p) => Some(p),
        _ => None,
    };
    let mut conn = SmtpConnection::connect(
        (connect.host.as_str(), connect.port),
        None,
        &connect.hello,
        wrapper,
        None,
    )?;
    let result = submit(&mut conn, connect, envelope, raw);
    match &result {
        Ok(_) => {
            let _ = conn.quit();
        }
        Err(_) => conn.abort(),
    }
    result
}

fn submit(
    conn: &mut SmtpConnection,
    connect: &Connect,
    envelope: &Envelope,
    raw: &[u8],
) -> Result<bool, Error> {
    if let Tls::Required(p) = &connect.tls {
        conn.starttls(p, &connect.hello)?;
    }
    // lettre keeps only the extensions it knows; ask again to see DSN.
    // Before AUTH, so the repeated greeting cannot reset a login.
    let ehlo = conn.command(Ehlo::new(connect.hello.clone()))?;
    let dsn = offers_dsn(ehlo.message());
    conn.auth(DEFAULT_MECHANISMS, &connect.credentials)?;
    if !dsn {
        log::info!("smtp: server offers no DSN, sending without a delivery confirmation");
        conn.send(envelope, raw)?;
        return Ok(false);
    }
    let mut mail = vec![MailParameter::Other {
        keyword: "RET".to_string(),
        value: Some("HDRS".to_string()),
    }];
    let non_ascii = envelope
        .from()
        .into_iter()
        .chain(envelope.to())
        .any(|a| !AsRef::<str>::as_ref(a).is_ascii());
    if non_ascii && conn.server_info().supports_feature(Extension::SmtpUtfEight) {
        mail.push(MailParameter::SmtpUtfEight);
    }
    if !raw.is_ascii() && conn.server_info().supports_feature(Extension::EightBitMime) {
        mail.push(MailParameter::Body(MailBodyParameter::EightBitMime));
    }
    conn.command(Mail::new(envelope.from().cloned(), mail))?;
    for to in envelope.to() {
        conn.command(Rcpt::new(to.clone(), rcpt_params(to)))?;
    }
    conn.command(Data)?;
    conn.message(raw)?;
    Ok(true)
}

/// Whether an EHLO reply lists the DSN extension.
fn offers_dsn<'a>(mut lines: impl Iterator<Item = &'a str>) -> bool {
    lines.any(|l| {
        l.split_whitespace()
            .next()
            .is_some_and(|w| w.eq_ignore_ascii_case("DSN"))
    })
}

/// `NOTIFY` for every outcome, and `ORCPT` with the address as given so
/// the report names it even after forwarding. lettre xtext-encodes values.
fn rcpt_params(to: &Address) -> Vec<RcptParameter> {
    let mut params = vec![RcptParameter::Other {
        keyword: "NOTIFY".to_string(),
        value: Some("SUCCESS,FAILURE,DELAY".to_string()),
    }];
    let addr: &str = to.as_ref();
    if addr.is_ascii() {
        params.push(RcptParameter::Other {
            keyword: "ORCPT".to_string(),
            value: Some(format!("rfc822;{addr}")),
        });
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsn_is_found_in_the_ehlo_reply() {
        let reply = ["mail.example.com Hello", "PIPELINING", "DSN", "8BITMIME"];
        assert!(offers_dsn(reply.into_iter()));
        assert!(offers_dsn(["x", "dsn"].into_iter()));
        assert!(!offers_dsn(
            ["mail.example.com", "DSNX", "SIZE 1000"].into_iter()
        ));
    }

    #[test]
    fn recipients_ask_for_every_outcome_with_the_original_address() {
        let to: Address = "a+b=c@example.org".parse().unwrap();
        let params: Vec<String> = rcpt_params(&to).iter().map(|p| p.to_string()).collect();
        assert_eq!(
            params,
            [
                "NOTIFY=SUCCESS,FAILURE,DELAY",
                "ORCPT=rfc822;a+2Bb+3Dc@example.org"
            ]
        );
    }
}
