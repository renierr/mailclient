//! Recipient address handling: lenient `To` parsing, strict `Cc`/`Bcc`,
//! `Reply-To`, sender-domain alignment, and group-name fallback.

use crate::error::{Result, StoreError};

/// Split the To field into real mailboxes: entries that parse become the
/// `To` header, anything else (placeholder text for BCC-only sends) is
/// ignored — the envelope comes from whichever of To/Cc/Bcc parsed.
#[must_use]
pub fn valid_mailboxes(raw: &[String]) -> Vec<lettre::message::Mailbox> {
    raw.iter().filter_map(|s| s.parse().ok()).collect()
}

/// Split a Cc/Bcc field into real mailboxes, rejecting anything that does
/// not parse. Unlike To (which tolerates placeholder text for BCC-only
/// sends), a mistyped Cc/Bcc must fail loudly — silently dropping it would
/// lie about delivery. Display names (`Bob <bob@example.com>`) are fine;
/// the envelope later uses the bare address.
pub fn strict_mailboxes(field: &str, raw: &[String]) -> Result<Vec<lettre::message::Mailbox>> {
    raw.iter()
        .map(|s| {
            s.parse()
                .map_err(|_| StoreError::InvalidInput(format!("invalid address in {field}: {s}")))
        })
        .collect()
}

/// Parse the composer's optional Reply-To into a single mailbox: empty =
/// no header (`None`), anything unparseable is a user-facing error (fail
/// here, not as a silent missing header after send).
pub fn parse_reply_to(raw: &str) -> Result<Option<lettre::message::Mailbox>> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(None);
    }
    t.parse::<lettre::message::Mailbox>()
        .map(Some)
        .map_err(|_| StoreError::InvalidInput(format!("invalid Reply-To address: {t}")))
}

/// Whether the visible sender stays within the configured account domain.
#[must_use]
pub fn sender_domain_is_aligned(from: &str, account_email: &str) -> bool {
    let Some((_, from_domain)) = from.rsplit_once('@') else {
        return false;
    };
    let Some((_, account_domain)) = account_email.rsplit_once('@') else {
        return false;
    };
    !from_domain.is_empty()
        && !account_domain.is_empty()
        && from_domain.eq_ignore_ascii_case(account_domain)
}

/// Group display name for a BCC-only `To:` header (`Friends:;`): RFC 5322
/// `display-name` without specials that would break parsing, ASCII only.
/// Anything else (blank, punctuation-heavy, non-ASCII) falls back to the
/// standard `undisclosed-recipients` group — so recipients always see a
/// proper To line instead of a missing header.
#[must_use]
pub fn to_group_name(text: &str) -> String {
    let t = text.trim();
    let ok = !t.is_empty()
        && t.is_ascii()
        && !t.starts_with(' ')
        && !t.ends_with(' ')
        && !t.contains("  ")
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || "!#$%&'*+-/=?^_`{|}~".contains(c));
    if ok {
        t.to_string()
    } else {
        "undisclosed-recipients".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cc_bcc_validation_is_strict() {
        // Bare and display-name forms pass, envelope uses the bare address.
        let ok = strict_mailboxes(
            "Cc",
            &[
                "bob@example.com".to_string(),
                "Bob <bob2@example.com>".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(ok.len(), 2);
        assert_eq!(ok[1].email.to_string(), "bob2@example.com");
        // Garbage fails loudly instead of being dropped or sent raw.
        assert!(strict_mailboxes("Cc", &["bob@".to_string()]).is_err());
        assert!(strict_mailboxes("Bcc", &["".to_string()]).is_err());
    }

    #[test]
    fn to_field_tolerates_placeholder_text() {
        let s = |x: &str| x.to_string();
        // Real addresses pass through; placeholder text is dropped so a
        // BCC-only send carries no To header.
        let boxes = valid_mailboxes(&[s("bob@example.com"), s("my friends"), s("")]);
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].email.to_string(), "bob@example.com");
        assert!(valid_mailboxes(&[s("anything goes"), s("")]).is_empty());
        assert!(valid_mailboxes(&[]).is_empty());
    }

    #[test]
    fn to_group_name_carries_safe_text() {
        // Plain placeholder text becomes the group display name…
        assert_eq!(to_group_name("my friends"), "my friends");
        assert_eq!(to_group_name("  Family  "), "Family");
        // …anything else falls back to the standard group.
        assert_eq!(to_group_name(""), "undisclosed-recipients");
        assert_eq!(to_group_name("a@b.com, c@d.org"), "undisclosed-recipients");
        assert_eq!(to_group_name("weird; text"), "undisclosed-recipients");
        assert_eq!(to_group_name("Müller"), "undisclosed-recipients");
        assert_eq!(to_group_name("a\r\nBcc: x@y"), "undisclosed-recipients");
    }

    #[test]
    fn sender_domain_must_match_the_account_domain() {
        assert!(sender_domain_is_aligned(
            "alias@example.com",
            "me@example.com"
        ));
        assert!(sender_domain_is_aligned(
            "alias@EXAMPLE.COM",
            "me@example.com"
        ));
        assert!(!sender_domain_is_aligned(
            "alias@other.example",
            "me@example.com"
        ));
        assert!(!sender_domain_is_aligned("alias", "me@example.com"));
        assert!(!sender_domain_is_aligned("alias@example.com", "me"));
    }
}
