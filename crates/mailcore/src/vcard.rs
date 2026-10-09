//! Parsing vCards (`.vcf`, RFC 6350 plus the 2.1 / 3.0 forms still sent by
//! phones and Outlook) for the reader's contact preview card.
//!
//! Reads the first `VCARD` of a file and counts the rest. Shares the
//! content-line grammar with `calendar`; adds what vCard needs on top:
//! property groups (`item1.EMAIL`), bare 2.1 type parameters (`TEL;CELL:`),
//! quoted-printable values with soft line breaks, and `;`-structured values
//! (`N`, `ADR`, `ORG`). Photos and other binary values are ignored.

use serde::{Deserialize, Serialize};

use crate::content_line::{parse_content_line, unescape_text, unfold, ContentLine};

/// Entries per list kept on the card; a card is a preview, not an editor.
const MAX_ENTRIES: usize = 8;

/// Largest `.vcf` parsed for a preview card. An explicit download stores
/// parts up to 25 MiB and the feed re-parses them on every open; this
/// still takes an address-book export of a few thousand cards. A larger
/// file stays an ordinary attachment (truncating it would miscount
/// `more_cards`).
const MAX_VCARD_BYTES: usize = 4 * 1024 * 1024;

/// One e-mail address or phone number with its kind (`Work`, `Mobile`,
/// `Home fax`, …), when the card names one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactField {
    pub value: String,
    pub label: Option<String>,
}

/// Parsed contact: exactly what the reader's contact card shows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContactCard {
    /// Display name: `FN`, else the parts of `N`, else the organisation or
    /// first address. While not `loaded`, the attachment's name.
    pub name: String,
    /// Job title and organisation as one line (`Title · Org, Unit`).
    pub affiliation: Option<String>,
    pub emails: Vec<ContactField>,
    pub phones: Vec<ContactField>,
    /// The preferred (else first) postal address on one line.
    pub address: Option<String>,
    pub url: Option<String>,
    /// Further cards in the same file that the card does not show.
    pub more_cards: usize,
    /// False while the attachment's bytes are not downloaded: only `name`
    /// and the attachment fields are set then.
    pub loaded: bool,
    /// The `.vcf` attachment the card came from.
    pub attachment_id: Option<i64>,
    /// Filesystem-safe name to open or save that attachment under.
    pub save_name: Option<String>,
}

impl ContactCard {
    /// Link the card to the attachment it was parsed from.
    pub fn set_attachment(&mut self, att: &crate::models::Attachment) {
        self.attachment_id = Some(att.id);
        self.save_name = Some(crate::paths::safe_attachment_name_for_mime(
            att.filename.as_deref(),
            att.mime_type.as_deref(),
            att.id,
        ));
    }

    /// Placeholder for a `.vcf` attachment whose bytes are not cached yet.
    pub fn pending(att: &crate::models::Attachment) -> Self {
        let name = att
            .filename
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or("Contact")
            .to_string();
        let mut card = ContactCard {
            name,
            affiliation: None,
            emails: Vec::new(),
            phones: Vec::new(),
            address: None,
            url: None,
            more_cards: 0,
            loaded: false,
            attachment_id: None,
            save_name: None,
        };
        card.set_attachment(att);
        card
    }
}

/// Whether an attachment is a contact card, by MIME or `.vcf` / `.vcard` name.
pub fn is_vcard_attachment(filename: Option<&str>, mime: Option<&str>) -> bool {
    let by_name = filename.is_some_and(|f| {
        let f = f.trim().to_ascii_lowercase();
        f.ends_with(".vcf") || f.ends_with(".vcard")
    });
    let by_mime = mime.is_some_and(|m| crate::mime::canonical_mime(m) == "text/vcard");
    by_name || by_mime
}

/// Parse vCard bytes. UTF-8 is expected; anything else is read as Latin-1,
/// which old phone exports use.
pub fn parse_vcard_bytes(bytes: &[u8]) -> Option<ContactCard> {
    if bytes.len() > MAX_VCARD_BYTES {
        return None;
    }
    parse_vcard(&decode_text(bytes))
}

/// Parse the first `VCARD` of `data`. `None` when there is none.
pub fn parse_vcard(data: &str) -> Option<ContactCard> {
    if data.len() > MAX_VCARD_BYTES {
        return None;
    }
    let unfolded = unfold(data);
    let mut lines = unfolded.lines();
    let mut depth = 0usize;
    let mut cards = 0usize;
    let mut b = Builder::default();

    while let Some(raw) = lines.next() {
        let raw = raw.trim_end();
        if raw.trim().is_empty() {
            continue;
        }
        let Some(cl) = parse_content_line(raw) else {
            continue;
        };
        let name = property_name(&cl.name);
        match name {
            "BEGIN" if cl.value.trim().eq_ignore_ascii_case("VCARD") => {
                if depth == 0 {
                    cards += 1;
                }
                depth += 1;
                continue;
            }
            "END" if cl.value.trim().eq_ignore_ascii_case("VCARD") => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        // Only the first card's own properties; a nested (embedded agent)
        // card is skipped.
        if cards != 1 || depth != 1 {
            continue;
        }
        let value = if is_quoted_printable(&cl) {
            let mut joined = cl.value.to_string();
            // A soft line break (`=` at the end) continues on the next
            // physical line, without the leading blank folding needs.
            while joined.ends_with('=') {
                let Some(next) = lines.next() else { break };
                joined.pop();
                joined.push_str(next.trim_end());
            }
            decode_text(&decode_quoted_printable(&joined))
        } else {
            cl.value.to_string()
        };
        b.read(name, &cl, &value);
    }

    if cards == 0 {
        return None;
    }
    Some(b.finish(cards - 1))
}

#[derive(Default)]
struct Builder {
    formatted_name: Option<String>,
    structured_name: Option<String>,
    organization: Option<String>,
    title: Option<String>,
    emails: Vec<ContactField>,
    phones: Vec<ContactField>,
    address: Option<(bool, String)>,
    url: Option<String>,
}

impl Builder {
    fn read(&mut self, name: &str, cl: &ContentLine<'_>, value: &str) {
        match name {
            "FN" => set_once(&mut self.formatted_name, text(value)),
            "N" => set_once(&mut self.structured_name, structured_name(value)),
            "ORG" => set_once(&mut self.organization, join_components(value, ", ")),
            "TITLE" => set_once(&mut self.title, text(value)),
            "EMAIL" => {
                let addr = text(value).map(|v| strip_scheme(&v, "mailto:").to_string());
                push_field(&mut self.emails, addr, label(cl, false));
            }
            "TEL" => {
                let number = text(value).map(|v| strip_scheme(&v, "tel:").to_string());
                push_field(&mut self.phones, number, label(cl, true));
            }
            "ADR" => {
                let Some(addr) = address(value) else { return };
                let preferred = types(cl).iter().any(|t| t == "pref")
                    || cl.param("PREF").is_some_and(|p| p.trim() == "1");
                if self.address.as_ref().is_none_or(|(p, _)| preferred && !p) {
                    self.address = Some((preferred, addr));
                }
            }
            "URL" => set_once(&mut self.url, text(value)),
            _ => {}
        }
    }

    fn finish(self, more_cards: usize) -> ContactCard {
        let affiliation = match (self.title, self.organization) {
            (Some(t), Some(o)) => Some(format!("{t} · {o}")),
            (t, o) => t.or(o),
        };
        let name = self
            .formatted_name
            .or(self.structured_name)
            .or_else(|| affiliation.clone())
            .or_else(|| self.emails.first().map(|e| e.value.clone()))
            .unwrap_or_else(|| "(Contact)".to_string());
        // A name that is only the organisation need not repeat below it.
        let affiliation = affiliation.filter(|a| *a != name);
        ContactCard {
            name,
            affiliation,
            emails: self.emails,
            phones: self.phones,
            address: self.address.map(|(_, a)| a),
            url: self.url,
            more_cards,
            loaded: true,
            attachment_id: None,
            save_name: None,
        }
    }
}

/// The property name without its group (`item1.EMAIL` → `EMAIL`).
fn property_name(name: &str) -> &str {
    name.rsplit('.').next().unwrap_or(name)
}

fn set_once(slot: &mut Option<String>, value: Option<String>) {
    if slot.is_none() {
        *slot = value;
    }
}

/// Unescaped, trimmed text; `None` when blank.
fn text(value: &str) -> Option<String> {
    Some(unescape_text(value).trim().to_string()).filter(|s| !s.is_empty())
}

fn strip_scheme<'a>(value: &'a str, scheme: &str) -> &'a str {
    match value.get(..scheme.len()) {
        Some(p) if p.eq_ignore_ascii_case(scheme) => value[scheme.len()..].trim(),
        _ => value,
    }
}

fn push_field(list: &mut Vec<ContactField>, value: Option<String>, label: Option<String>) {
    let Some(value) = value else { return };
    if list.len() >= MAX_ENTRIES || list.iter().any(|f| f.value.eq_ignore_ascii_case(&value)) {
        return;
    }
    list.push(ContactField { value, label });
}

/// Split a structured value at each `;` not escaped by a backslash, and
/// unescape each part.
fn components(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                current.push('\\');
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ';' => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
        .iter()
        .map(|p| unescape_text(p).trim().to_string())
        .collect()
}

fn join_components(value: &str, sep: &str) -> Option<String> {
    let joined = components(value)
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(sep);
    Some(joined).filter(|j| !j.is_empty())
}

/// `N:Family;Given;Additional;Prefix;Suffix` in reading order.
fn structured_name(value: &str) -> Option<String> {
    let c = components(value);
    let part = |i: usize| c.get(i).map(String::as_str).unwrap_or("");
    let ordered = [part(3), part(1), part(2), part(0), part(4)];
    let joined = ordered
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    Some(joined).filter(|j| !j.is_empty())
}

/// `ADR:PO box;Extended;Street;Locality;Region;Postal code;Country` on one
/// line: street parts first, then `code locality`, region and country.
fn address(value: &str) -> Option<String> {
    let c = components(value);
    let part = |i: usize| c.get(i).map(String::as_str).unwrap_or("");
    let town = [part(5), part(3)]
        .iter()
        .filter(|p| !p.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    let lines = [part(2), part(1), part(0), &town, part(4), part(6)];
    let joined = lines
        .iter()
        .map(|p| p.replace('\n', ", "))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    Some(joined).filter(|j| !j.is_empty())
}

/// Lowercased type values: `TYPE=work,voice`, repeated `TYPE`s, and the
/// bare 2.1 form (`TEL;WORK;CELL:`).
fn types(cl: &ContentLine<'_>) -> Vec<String> {
    let mut out = Vec::new();
    for (name, value) in &cl.params {
        if name == "TYPE" {
            out.extend(value.split(',').map(|t| t.trim().to_ascii_lowercase()));
        } else if value.is_empty() {
            out.push(name.to_ascii_lowercase());
        }
    }
    out
}

/// `Work`, `Home`, `Mobile`, `Work fax`, … from the type parameters.
fn label(cl: &ContentLine<'_>, phone: bool) -> Option<String> {
    let t = types(cl);
    let has = |n: &str| t.iter().any(|x| x == n);
    let place = if has("work") {
        Some("Work")
    } else if has("home") {
        Some("Home")
    } else {
        None
    };
    let kind = if !phone {
        None
    } else if has("cell") || has("mobile") {
        Some("mobile")
    } else if has("fax") {
        Some("fax")
    } else if has("pager") {
        Some("pager")
    } else {
        None
    };
    match (place, kind) {
        (Some(p), Some(k)) => Some(format!("{p} {k}")),
        (Some(p), None) => Some(p.to_string()),
        (None, Some(k)) => {
            let mut s = k.to_string();
            // `get_mut(..1)`, not `[..1]`: the kind is always an ASCII label
            // today, but a future caller handing it one that starts with a
            // multi-byte character would panic on the byte index — the same
            // class of bug as `badge.rs`'s punycode check.
            if let Some(c) = s.get_mut(..1) {
                c.make_ascii_uppercase();
            }
            Some(s)
        }
        (None, None) => None,
    }
}

fn is_quoted_printable(cl: &ContentLine<'_>) -> bool {
    cl.params.iter().any(|(n, v)| {
        (n == "ENCODING" && v.eq_ignore_ascii_case("QUOTED-PRINTABLE"))
            || (n == "QUOTED-PRINTABLE" && v.is_empty())
    })
}

fn decode_quoted_printable(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok());
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// UTF-8 when valid, else Latin-1 (each byte is its code point).
fn decode_text(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

#[cfg(test)]
mod tests;
