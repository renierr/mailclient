//! Where a reply goes. Both readers warn when Reply-To points away from the
//! sender and both composers address the reply; this decides it once.

/// The address a reply is sent to, and whether that is not the sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyAddress {
    /// Reply-To when the mail sets one, else From.
    pub target: String,
    /// Reply-To is set and names another address than From, so answering
    /// does not reach the sender. The UI says so out loud: replying to the
    /// wrong address cannot be taken back.
    pub differs: bool,
}

/// Compares bare addresses (`Name <a@b>` reduces to `a@b`), trimmed and
/// case-insensitively.
pub fn reply_address(from: &str, reply_to: &str) -> ReplyAddress {
    let reply_to = reply_to.trim();
    if reply_to.is_empty() {
        return ReplyAddress {
            target: from.trim().to_string(),
            differs: false,
        };
    }
    ReplyAddress {
        target: reply_to.to_string(),
        differs: !bare(reply_to).eq_ignore_ascii_case(bare(from)),
    }
}

/// The address inside `Name <addr>`, or the whole trimmed string.
pub(super) fn bare(addr: &str) -> &str {
    let s = addr.trim();
    match (s.find('<'), s.rfind('>')) {
        (Some(lt), Some(gt)) if gt > lt => s[lt + 1..gt].trim(),
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_reply_to_the_sender_is_the_target() {
        let r = reply_address("alice@example.com", " ");
        assert_eq!(r.target, "alice@example.com");
        assert!(!r.differs);
    }

    #[test]
    fn same_address_in_another_spelling_does_not_differ() {
        let r = reply_address("Alice@Example.com", "Alice <alice@example.com>");
        assert!(!r.differs);
        assert_eq!(r.target, "Alice <alice@example.com>");
    }

    #[test]
    fn another_address_differs_and_is_the_target() {
        let r = reply_address("alice@example.com", "list@example.org");
        assert_eq!(r.target, "list@example.org");
        assert!(r.differs);
    }
}
