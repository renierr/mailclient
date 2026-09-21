//! Finding out what the server has: LIST / LSUB / NAMESPACE, plus the
//! NOOP that doubles as a liveness probe for pooled sessions.

use super::*;

impl ImapSession {
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

    /// NOOP health check.
    pub async fn noop(&mut self) -> Result<()> {
        self.execute(CommandBody::Noop).await?;
        Ok(())
    }
}
