//! What the account setup forms fill in, offer and check before saving.
//!
//! Both forms used to carry their own copy: different host guesses
//! (`imap.` vs `mail.`), different security choices, and port swaps written
//! twice. The forms now ask here and only lay the answers out.

use serde_json::{json, Map, Value};

/// Which server a port or security setting belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Imap,
    Smtp,
}

impl Protocol {
    /// `"imap"` / `"smtp"`, anything else is `None`.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "imap" => Some(Self::Imap),
            "smtp" => Some(Self::Smtp),
            _ => None,
        }
    }
}

/// The security settings the forms offer, in display order. `none` is the
/// explicit plaintext opt-in; the forms warn while it is chosen.
pub const SECURITY_CHOICES: [&str; 3] = ["tls", "starttls", "none"];

/// The stored security value for whatever an older build or a hand edit
/// left: `ssl` is `tls`, `plain` is `none`, blank or unknown is `tls`.
#[must_use]
pub fn normalize_security(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "starttls" => "starttls",
        "none" | "plain" => "none",
        _ => "tls",
    }
}

/// Whether `security` sends password and mail unencrypted.
#[must_use]
pub fn is_plaintext(security: &str) -> bool {
    normalize_security(security) == "none"
}

/// The usual port for a protocol under a security setting.
#[must_use]
pub fn default_port(protocol: Protocol, security: &str) -> u16 {
    match (protocol, normalize_security(security)) {
        (Protocol::Imap, "tls") => 993,
        (Protocol::Imap, _) => 143,
        (Protocol::Smtp, "tls") => 465,
        (Protocol::Smtp, _) => 587,
    }
}

/// The port field after the security setting changed from `old` to `new`:
/// a blank port or the usual one for `old` follows to the usual one for
/// `new`; a port the user chose stays.
#[must_use]
pub fn port_after_security_change(protocol: Protocol, old: &str, new: &str, port: &str) -> String {
    let port = port.trim();
    if port.is_empty() || port == default_port(protocol, old).to_string() {
        default_port(protocol, new).to_string()
    } else {
        port.to_string()
    }
}

/// Server guesses for a typed address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerGuess {
    pub imap_host: String,
    pub smtp_host: String,
    pub imap_user: String,
}

/// `imap.` / `smtp.` plus the address's domain, the address as user.
/// `None` until the address has a local part and a dotted domain.
#[must_use]
pub fn guess(email: &str) -> Option<ServerGuess> {
    let email = email.trim();
    let (local, domain) = email.split_once('@')?;
    let dotted = domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.');
    if local.is_empty() || !dotted || domain.contains('@') {
        return None;
    }
    Some(ServerGuess {
        imap_host: format!("imap.{domain}"),
        smtp_host: format!("smtp.{domain}"),
        imap_user: email.to_string(),
    })
}

/// What a new form starts with, and the choices it offers.
#[must_use]
pub fn defaults_json() -> String {
    json!({
        "imap_sec": "tls",
        "imap_port": default_port(Protocol::Imap, "tls").to_string(),
        "smtp_sec": "tls",
        "smtp_port": default_port(Protocol::Smtp, "tls").to_string(),
        "security_choices": SECURITY_CHOICES,
    })
    .to_string()
}

/// Per-field problems with a form, as the forms show them inline.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FormCheck {
    /// Field key → why it cannot be saved. Empty means the form saves.
    pub errors: Vec<(&'static str, String)>,
    /// Field key → something to know that does not block saving.
    pub warnings: Vec<(&'static str, String)>,
}

impl FormCheck {
    /// `{"errors": {field: text}, "warnings": {field: text}}`.
    #[must_use]
    pub fn to_json(&self) -> String {
        let map = |items: &[(&str, String)]| {
            items
                .iter()
                .map(|(k, v)| ((*k).to_string(), Value::String(v.clone())))
                .collect::<Map<_, _>>()
        };
        json!({ "errors": map(&self.errors), "warnings": map(&self.warnings) }).to_string()
    }
}

/// Check a form the way [`super::save`] will. `editing` relaxes the
/// password: an edit keeps the stored one when the field is blank.
#[must_use]
pub fn check(form: &Value, editing: bool) -> FormCheck {
    let text = |k: &str| {
        form.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let mut out = FormCheck::default();
    let email = text("email");
    if email.is_empty() {
        out.errors.push(("email", "Enter the email address".into()));
    } else if !email
        .split_once('@')
        .is_some_and(|(l, d)| !l.is_empty() && !d.is_empty())
    {
        out.errors
            .push(("email", "Enter a full email address".into()));
    }
    for (key, what) in [("imap_host", "IMAP"), ("smtp_host", "SMTP")] {
        if text(key).is_empty() {
            out.errors.push((key, format!("Enter the {what} server")));
        }
    }
    for key in ["imap_port", "smtp_port"] {
        if port_value(form.get(key)).is_err() {
            out.errors
                .push((key, "A port is a number from 1 to 65535".into()));
        }
    }
    if !editing
        && form
            .get("password")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .is_empty()
    {
        out.errors.push(("password", "Enter the password".into()));
    }
    for (key, what) in [("imap_sec", "Incoming"), ("smtp_sec", "Outgoing")] {
        if is_plaintext(&text(key)) {
            out.warnings.push((
                key,
                format!(
                    "{what} mail is not encrypted: your password and messages \
                     travel in plain text. Use only on a network you trust."
                ),
            ));
        }
    }
    out
}

/// A port field: a string from a text field or a number, blank meaning the
/// default (`None`). Anything else is refused rather than replaced.
pub(super) fn port_value(v: Option<&Value>) -> Result<Option<u16>, ()> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(x) => x
            .as_str()
            .and_then(|s| s.trim().parse().ok())
            .or_else(|| x.as_u64().and_then(|n| u16::try_from(n).ok()))
            .filter(|p| *p > 0)
            .map(Some)
            .ok_or(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guess_uses_imap_and_smtp_hosts_and_the_address_as_user() {
        let g = guess(" user@example.com ").unwrap();
        assert_eq!(g.imap_host, "imap.example.com");
        assert_eq!(g.smtp_host, "smtp.example.com");
        assert_eq!(g.imap_user, "user@example.com");
        for half_typed in [
            "user",
            "user@",
            "@example.com",
            "user@example",
            "user@example.",
        ] {
            assert_eq!(guess(half_typed), None, "{half_typed}");
        }
    }

    #[test]
    fn ports_follow_security_unless_the_user_chose_one() {
        use Protocol::*;
        assert_eq!(
            port_after_security_change(Imap, "tls", "starttls", "993"),
            "143"
        );
        assert_eq!(
            port_after_security_change(Imap, "starttls", "tls", "143"),
            "993"
        );
        assert_eq!(
            port_after_security_change(Smtp, "tls", "none", "465"),
            "587"
        );
        assert_eq!(
            port_after_security_change(Smtp, "tls", "starttls", ""),
            "587"
        );
        assert_eq!(
            port_after_security_change(Imap, "tls", "starttls", "1993"),
            "1993"
        );
    }

    #[test]
    fn older_security_values_are_normalized() {
        assert_eq!(normalize_security("SSL"), "tls");
        assert_eq!(normalize_security("plain"), "none");
        assert_eq!(normalize_security(""), "tls");
        assert_eq!(normalize_security("StartTLS"), "starttls");
        assert!(is_plaintext("plain") && !is_plaintext("starttls"));
    }

    #[test]
    fn the_check_names_each_field_and_warns_on_plaintext() {
        let form = json!({"email": "user", "imap_port": "99999", "smtp_sec": "none"});
        let c = check(&form, false);
        let keys: Vec<_> = c.errors.iter().map(|(k, _)| *k).collect();
        assert_eq!(
            keys,
            ["email", "imap_host", "smtp_host", "imap_port", "password"]
        );
        assert_eq!(c.warnings.len(), 1);
        assert_eq!(c.warnings[0].0, "smtp_sec");

        let ok = json!({"email": "user@example.com", "imap_host": "imap.example.com",
            "smtp_host": "smtp.example.com", "imap_port": 993});
        assert!(
            check(&ok, true).errors.is_empty(),
            "an edit keeps the password"
        );
        assert_eq!(check(&ok, false).errors[0].0, "password");
    }
}
