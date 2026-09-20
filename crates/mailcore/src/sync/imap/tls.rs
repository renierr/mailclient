//! TLS: rustls connector (system + WebPKI roots) and server-name resolution.

use std::{net::IpAddr, sync::Arc};

use rustls_pki_types::ServerName;
use tokio_rustls::{rustls, TlsConnector};

use crate::error::{Result, StoreError};

/// Helper to build a TLS connector trusting system certificates with WebPKI roots fallback.
pub(crate) fn build_tls_connector() -> Result<TlsConnector> {
    let mut root_store = rustls::RootCertStore::empty();
    let native_certs = rustls_native_certs::load_native_certs();
    for cert in native_certs.certs {
        let _ = root_store.add(cert);
    }
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}
/// Resolve a rustls [`ServerName`] for `host`, supporting DNS names and
/// IP literals (test mocks dial `127.0.0.1`, which plain `try_from` rejects).
pub(crate) fn server_name_for(host: &str) -> Result<ServerName<'static>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(ServerName::IpAddress(ip.into()));
    }
    ServerName::try_from(host.to_string())
        .map(|s| s.to_owned())
        .map_err(|e| StoreError::Network(format!("invalid server name {host}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_name_accepts_dns_and_ip_literals() {
        assert!(server_name_for("imap.example.com").is_ok());
        assert!(server_name_for("127.0.0.1").is_ok());
        assert!(server_name_for("::1").is_ok());
        assert!(server_name_for("").is_err());
        assert!(server_name_for("bad host!").is_err());
    }

    #[test]
    fn tls_connector_builds() {
        assert!(build_tls_connector().is_ok());
    }
}
