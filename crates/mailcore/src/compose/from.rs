//! The composer's From field. The domain is locked to the account: only the
//! local part edits, since sending as another domain breaks SPF and
//! domain-aligned DKIM/DMARC. Both composers split and join the address
//! here, so the address shown is the address sent.

use serde::Serialize;

/// An address split for the From field: the editable local part and the
/// fixed domain suffix, shown with its `@` (`"@example.com"`, or `""` when
/// the address has no domain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SenderParts {
    pub local: String,
    pub domain: String,
}

/// Split at the last `@`, the way [`sender_domain_is_aligned`] reads it.
///
/// [`sender_domain_is_aligned`]: crate::sync::sender::sender_domain_is_aligned
pub fn sender_parts(address: &str) -> SenderParts {
    let address = address.trim();
    match address.rfind('@') {
        Some(at) => SenderParts {
            local: address[..at].to_string(),
            domain: address[at..].to_string(),
        },
        None => SenderParts {
            local: address.to_string(),
            domain: String::new(),
        },
    }
}

/// The address a From field sends as: the typed local part on the account's
/// domain, or the account address when the field is blank. A domain typed
/// into the field is dropped — the domain is the account's, whatever was
/// typed — so a full address pasted in cannot slip past the lock.
pub fn effective_from(local: &str, account_email: &str) -> String {
    let local = local.trim();
    let local = local.split('@').next().unwrap_or_default().trim();
    if local.is_empty() {
        return account_email.trim().to_string();
    }
    let domain = sender_parts(account_email).domain;
    format!("{local}{domain}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_split_at_the_last_at() {
        assert_eq!(
            sender_parts(" me@example.com "),
            SenderParts {
                local: "me".into(),
                domain: "@example.com".into()
            }
        );
        assert_eq!(sender_parts("\"a@b\"@example.com").local, "\"a@b\"");
        assert_eq!(sender_parts("me").domain, "");
    }

    #[test]
    fn blank_field_sends_as_the_account() {
        assert_eq!(effective_from("  ", "me@example.com"), "me@example.com");
    }

    #[test]
    fn local_part_joins_the_account_domain() {
        assert_eq!(
            effective_from(" sales ", "me@example.com"),
            "sales@example.com"
        );
    }

    #[test]
    fn typed_domain_is_replaced_by_the_account_domain() {
        assert_eq!(
            effective_from("sales@example.org", "me@example.com"),
            "sales@example.com"
        );
        assert_eq!(
            effective_from("@example.org", "me@example.com"),
            "me@example.com"
        );
    }

    #[test]
    fn account_without_domain_keeps_the_local_part() {
        assert_eq!(effective_from("sales", "me"), "sales");
    }
}
