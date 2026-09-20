//! Send policy and format: who may receive automated mail and which MIME
//! shape an outbound message takes.

use crate::error::{Result, StoreError};

/// Recipient policy enforced before every send.
#[derive(Debug, Clone)]
pub enum SendPolicy {
    /// Only these (lowercased) addresses may receive mail.
    TestAllowlist(Vec<String>),
    /// No restrictions (production, explicit opt-in).
    Unrestricted,
}

impl SendPolicy {
    /// Build from the environment (see module docs).
    /// Unset/empty allowlist denies every recipient.
    ///
    /// `MAILCLIENT_ALLOW_ANY_RECIPIENT=1` lifts the restriction for automated
    /// sends (test harness only — the interactive composer does not consult
    /// this at all). Never export it globally: it belongs in the local
    /// gitignored `.env`, if anywhere.
    #[must_use]
    pub fn from_env() -> Self {
        if std::env::var("MAILCLIENT_ALLOW_ANY_RECIPIENT").as_deref() == Ok("1") {
            return Self::Unrestricted;
        }
        let raw = std::env::var("MAILCLIENT_TEST_SEND_ALLOWLIST").unwrap_or_default();
        let allow = raw
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        Self::TestAllowlist(allow)
    }

    /// Reject if any recipient is not allowlisted.
    pub fn check(&self, recipients: &[&str]) -> Result<()> {
        match self {
            Self::Unrestricted => Ok(()),
            Self::TestAllowlist(allow) => {
                for r in recipients {
                    if !allow.iter().any(|a| a == &r.to_ascii_lowercase()) {
                        return Err(StoreError::InvalidInput(format!(
                            "refusing to send to {r} (test allowlist: {allow:?})"
                        )));
                    }
                }
                Ok(())
            }
        }
    }
}

/// Outgoing body format (user setting `compose_send_format`, resilient).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendFormat {
    /// Smart default: plain text unless the body carries formatting, in
    /// which case HTML (plus a plain twin when `include_plain` is on).
    Auto,
    /// `text/plain` only — safest, always readable.
    Plain,
    /// `multipart/alternative` plain + html — resilient.
    Multipart,
    /// `text/html` only (plus a plain twin when `include_plain` is on).
    Html,
}

impl SendFormat {
    /// Parse user setting; unknown/empty → `Auto`.
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match crate::store::settings::normalize_send_format(raw) {
            "plain" => Self::Plain,
            "multipart" => Self::Multipart,
            "html" => Self::Html,
            _ => Self::Auto,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Plain => "plain",
            Self::Multipart => "multipart",
            Self::Html => "html",
        }
    }
}

/// Resolve the wanted format into a concrete wire shape (never `Auto`).
///
/// - `Plain` / `Multipart` pass through untouched.
/// - `Html` gains a plain twin (`Multipart`) when `include_plain` is on.
/// - `Auto`: plain text when the body has no formatting, otherwise HTML
///   (with a plain twin when `include_plain` is on).
#[must_use]
pub fn effective_format(wanted: SendFormat, needs_html: bool, include_plain: bool) -> SendFormat {
    match wanted {
        SendFormat::Auto if !needs_html => SendFormat::Plain,
        SendFormat::Auto | SendFormat::Html if include_plain => SendFormat::Multipart,
        SendFormat::Auto | SendFormat::Html => SendFormat::Html,
        concrete => concrete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_blocks_non_allowlisted_recipients() {
        let policy = SendPolicy::TestAllowlist(vec!["allowed@example.com".to_string()]);
        assert!(policy.check(&["allowed@example.com"]).is_ok());
        assert!(policy.check(&["ALLOWED@example.com"]).is_ok());
        assert!(policy.check(&["someone@else.example"]).is_err());
        assert!(policy
            .check(&["allowed@example.com", "evil@example.org"])
            .is_err());
        assert!(SendPolicy::TestAllowlist(vec![])
            .check(&["anyone@example.com"])
            .is_err());
        assert!(SendPolicy::Unrestricted
            .check(&["anyone@example.com"])
            .is_ok());
    }

    #[test]
    fn auto_format_picks_shape_from_content() {
        use SendFormat::{Auto, Html, Multipart, Plain};
        // Plain typing (even wrapped in editor structure) sends text/plain.
        assert_eq!(effective_format(Auto, false, true), Plain);
        assert_eq!(effective_format(Auto, false, false), Plain);
        // Formatting sends multipart by default, html-only on opt-out.
        assert_eq!(effective_format(Auto, true, true), Multipart);
        assert_eq!(effective_format(Auto, true, false), Html);
        // Explicit choices pass through; html gains a twin on opt-in.
        assert_eq!(effective_format(Plain, true, true), Plain);
        assert_eq!(effective_format(Multipart, false, false), Multipart);
        assert_eq!(effective_format(Html, true, true), Multipart);
        assert_eq!(effective_format(Html, true, false), Html);
    }

    #[test]
    fn needs_html_only_for_real_formatting() {
        use crate::html::{needs_html_formatting, sanitize_for_send};
        // Editor structure around plain typing: no HTML needed.
        assert!(!needs_html_formatting(&sanitize_for_send("<p>hello</p>")));
        assert!(!needs_html_formatting(&sanitize_for_send(
            "<div>one</div><div>two<br></div>"
        )));
        assert!(!needs_html_formatting(&sanitize_for_send(
            "plain &amp; simple"
        )));
        // Real formatting needs HTML.
        assert!(needs_html_formatting(&sanitize_for_send(
            "<p>hello <b>bold</b></p>"
        )));
        assert!(needs_html_formatting(&sanitize_for_send(
            "<p>see <a href=\"https://x.example\">this</a></p>"
        )));
        assert!(needs_html_formatting(&sanitize_for_send(
            "<ul><li>one</li></ul>"
        )));
        assert!(needs_html_formatting(&sanitize_for_send(
            "<blockquote>quoted</blockquote>"
        )));
        // A `>`-citation reply draft (what the composer emits for plain
        // mail) carries no formatting: Auto keeps it text/plain.
        assert!(!needs_html_formatting(&sanitize_for_send(
            "<p></p><p>&gt; quoted<br>&gt; more</p>"
        )));
    }
}
