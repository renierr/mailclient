//! New mail started from outside the app: a `mailto:` link (RFC 6068) or
//! something shared into it (Android's share sheet, `SENDTO` with extras).
//!
//! The adapter only collects what the platform handed over — the link and
//! any loose recipients, subject and text — into a [`PrefillRequest`]; the
//! merge, the link parsing and the body with the signature happen here, so
//! a desktop `mailto:` handler can reuse the same call. Files shared along
//! stay with the adapter: they are attachments, staged like picked files.

use serde::{Deserialize, Serialize};

use super::answer::{blank_draft, stored_options};
use crate::db::Db;
use crate::error::Result;

/// What the platform handed over. Every field is optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PrefillRequest {
    /// A `mailto:` URI, as tapped.
    pub mailto: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    /// Plain text (a shared link or note).
    pub text: String,
}

/// Recipients, subject and plain body of a [`PrefillRequest`] or a link.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prefill {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// The composer's starting point: recipients comma-joined like its fields,
/// `body_html` with the text above the signature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrefillDraft {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body_html: String,
}

/// Parse a `mailto:` URI: addresses before `?`, then `to`, `cc`, `bcc`,
/// `subject` and `body` header fields (names case-insensitive). Every other
/// field — `attach`, `from`, arbitrary headers — is ignored, as RFC 6068
/// advises for anything unsafe. `None` when it is not a `mailto:` URI.
#[must_use]
pub fn parse_mailto(uri: &str) -> Option<Prefill> {
    let uri = uri.trim();
    let (scheme, rest) = uri.split_once(':')?;
    if !scheme.eq_ignore_ascii_case("mailto") {
        return None;
    }
    // A raw `#` would be a fragment; RFC 6068 has no use for one.
    let rest = rest.split('#').next().unwrap_or_default();
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut out = Prefill {
        to: addresses(&decode(path)),
        ..Prefill::default()
    };
    let mut body: Option<String> = None;
    for field in query.split('&').filter(|f| !f.is_empty()) {
        let (name, value) = field.split_once('=').unwrap_or((field, ""));
        let value = decode(value);
        match decode(name).to_ascii_lowercase().as_str() {
            "to" => out.to.extend(addresses(&value)),
            "cc" => out.cc.extend(addresses(&value)),
            "bcc" => out.bcc.extend(addresses(&value)),
            "subject" if out.subject.is_empty() => out.subject = one_line(&value),
            "body" if body.is_none() => body = Some(value),
            _ => {}
        }
    }
    out.body = body.unwrap_or_default().replace("\r\n", "\n");
    Some(out)
}

/// Merge what the platform handed over: the link's recipients first, then
/// the loose ones, each address once (case-insensitive, across To, Cc and
/// Bcc); the link's subject unless it has none; its body, with shared text
/// that says something else below it.
#[must_use]
pub fn merge(req: &PrefillRequest) -> Prefill {
    let link = parse_mailto(&req.mailto).unwrap_or_default();
    let mut seen: Vec<String> = Vec::new();
    let mut unique = |a: Vec<String>, b: &[String]| -> Vec<String> {
        let mut out = Vec::new();
        for addr in a.into_iter().chain(b.iter().flat_map(|x| addresses(x))) {
            let key = bare(&addr).to_lowercase();
            if !seen.contains(&key) {
                seen.push(key);
                out.push(addr);
            }
        }
        out
    };
    let to = unique(link.to, &req.to);
    let cc = unique(link.cc, &req.cc);
    let bcc = unique(link.bcc, &req.bcc);
    let subject = if link.subject.trim().is_empty() {
        one_line(&req.subject)
    } else {
        link.subject
    };
    let text = req.text.replace("\r\n", "\n");
    let body = match (link.body.trim(), text.trim()) {
        ("", _) => text.trim().to_string(),
        (b, "") => b.to_string(),
        (b, t) if b == t => b.to_string(),
        (b, t) => format!("{b}\n\n{t}"),
    };
    Prefill {
        to,
        cc,
        bcc,
        subject,
        body,
    }
}

/// [`merge`] as the composer's starting point, with the stored signature.
pub fn prefill_draft(db: &Db, req: &PrefillRequest) -> PrefillDraft {
    let p = merge(req);
    let blank = blank_draft(&stored_options(db));
    PrefillDraft {
        to: p.to.join(", "),
        cc: p.cc.join(", "),
        bcc: p.bcc.join(", "),
        subject: p.subject,
        body_html: if p.body.is_empty() {
            blank.body_html
        } else {
            blank.with_text(&p.body)
        },
    }
}

/// [`prefill_draft`] from and to JSON, for the adapters.
pub fn prefill_draft_json(db: &Db, request: &str) -> Result<String> {
    let req: PrefillRequest = serde_json::from_str(request)?;
    Ok(serde_json::to_string(&prefill_draft(db, &req))?)
}

/// Comma-separated addresses, trimmed, empties dropped.
fn addresses(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(String::from)
        .collect()
}

/// The address inside `Name <addr>`, or the whole text.
fn bare(addr: &str) -> &str {
    match (addr.rfind('<'), addr.rfind('>')) {
        (Some(a), Some(b)) if a < b => addr[a + 1..b].trim(),
        _ => addr.trim(),
    }
}

/// A subject is one line: a link or share that breaks it gets spaces.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Percent-decoding as UTF-8 (invalid bytes become U+FFFD). `+` stays a
/// plus: RFC 6068 does not use form encoding.
fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::settings;

    #[test]
    fn parses_addresses_and_header_fields() {
        let p = parse_mailto(
            "mailto:a@example.com,b@example.com?cc=c@example.com&subject=Hello%20there&body=Line%201%0D%0ALine%202",
        )
        .unwrap();
        assert_eq!(p.to, vec!["a@example.com", "b@example.com"]);
        assert_eq!(p.cc, vec!["c@example.com"]);
        assert_eq!(p.subject, "Hello there");
        assert_eq!(p.body, "Line 1\nLine 2");
    }

    #[test]
    fn field_names_and_scheme_are_case_insensitive() {
        let p = parse_mailto("MAILTO:?To=a@example.com&BCC=b@example.com&Subject=x").unwrap();
        assert_eq!(p.to, vec!["a@example.com"]);
        assert_eq!(p.bcc, vec!["b@example.com"]);
        assert_eq!(p.subject, "x");
    }

    #[test]
    fn ignores_unsafe_fields_and_keeps_plus_literal() {
        let p =
            parse_mailto("mailto:a+tag@example.com?attach=/etc/passwd&from=x@example.org&body=1+1")
                .unwrap();
        assert_eq!(p.to, vec!["a+tag@example.com"]);
        assert_eq!(p.body, "1+1");
        assert_eq!(p.subject, "");
    }

    #[test]
    fn decodes_utf8_and_survives_broken_escapes() {
        let p = parse_mailto("mailto:a@example.com?subject=Gr%C3%BC%C3%9Fe&body=100%").unwrap();
        assert_eq!(p.subject, "Grüße");
        assert_eq!(p.body, "100%");
        let p = parse_mailto("mailto:?subject=%zz%C3").unwrap();
        assert_eq!(p.subject, "%zz\u{FFFD}");
    }

    #[test]
    fn subject_is_one_line() {
        let p = parse_mailto("mailto:?subject=a%0D%0Ab").unwrap();
        assert_eq!(p.subject, "a b");
    }

    #[test]
    fn not_a_mailto_link() {
        assert!(parse_mailto("https://example.com").is_none());
        assert!(parse_mailto("").is_none());
    }

    #[test]
    fn merge_dedupes_recipients_across_fields() {
        let req = PrefillRequest {
            mailto: "mailto:a@example.com?cc=b@example.com".into(),
            to: vec!["A@Example.com".into(), "Carol <c@example.com>".into()],
            bcc: vec!["b@example.com, d@example.com".into()],
            ..PrefillRequest::default()
        };
        let p = merge(&req);
        assert_eq!(p.to, vec!["a@example.com", "Carol <c@example.com>"]);
        assert_eq!(p.cc, vec!["b@example.com"]);
        assert_eq!(p.bcc, vec!["d@example.com"]);
    }

    #[test]
    fn merge_prefers_the_link_and_keeps_other_text() {
        let req = PrefillRequest {
            mailto: "mailto:a@example.com?subject=Link&body=From%20link".into(),
            subject: "Extra".into(),
            text: "Shared".into(),
            ..PrefillRequest::default()
        };
        let p = merge(&req);
        assert_eq!(p.subject, "Link");
        assert_eq!(p.body, "From link\n\nShared");

        let same = PrefillRequest {
            mailto: "mailto:?body=Same".into(),
            text: "Same".into(),
            ..PrefillRequest::default()
        };
        assert_eq!(merge(&same).body, "Same");

        let share_only = PrefillRequest {
            subject: "A page".into(),
            text: "https://example.com/page\n".into(),
            ..PrefillRequest::default()
        };
        let p = merge(&share_only);
        assert_eq!(p.subject, "A page");
        assert_eq!(p.body, "https://example.com/page");
    }

    #[test]
    fn draft_puts_the_text_above_the_signature_escaped() {
        let db = Db::open_in_memory().unwrap();
        settings::set(&db, settings::SIGNATURE_ENABLED, "1").unwrap();
        settings::set(&db, settings::SIGNATURE_TEXT, "Me").unwrap();
        let req = PrefillRequest {
            mailto: "mailto:a@example.com?body=%3Cb%3Ehi%3C%2Fb%3E%0A%0Asecond".into(),
            ..PrefillRequest::default()
        };
        let d = prefill_draft(&db, &req);
        assert_eq!(d.to, "a@example.com");
        assert!(
            d.body_html
                .starts_with("<p>&lt;b&gt;hi&lt;/b&gt;</p><p>second</p><p>-- "),
            "{}",
            d.body_html
        );
    }

    #[test]
    fn draft_without_text_is_the_blank_draft() {
        let db = Db::open_in_memory().unwrap();
        let json = prefill_draft_json(&db, r#"{"mailto":"mailto:a@example.com"}"#).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["to"], "a@example.com");
        assert_eq!(v["body_html"], "");
        assert!(prefill_draft_json(&db, "not json").is_err());
    }
}
