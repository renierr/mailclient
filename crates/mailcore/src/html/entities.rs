//! Entity decoding and escaping.
//!
//! Numeric entities, every named HTML 4 entity (see `entity_table`) and a
//! few HTML5 extras newsletters use. The serializer re-escapes text, so an
//! entity this misses reaches the reader literally (`&auml;` for `ä`).

/// How far past a `&` to look for the `;` that ends an entity. The bound is
/// what keeps the decode linear: it used to be a filter applied *after*
/// `s[i..].find(';')`, which scans to the end of the string before the
/// throwaway check, so any text full of ampersands without a following `;`
/// cost O(remaining) per `&` — 512 KB of that is ~9 s of CPU on every open.
const ENTITY_SCAN: usize = 23;

/// Decode character entities for URL decisions and text output.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'&' {
            // Bounded scan for the closing `;` (`i + 1` is in bounds even
            // when the `&` is the last byte, giving an empty slice).
            let semi = b[i + 1..]
                .iter()
                .take(ENTITY_SCAN)
                .position(|&c| c == b';')
                .map(|n| n + 1);
            if let Some(semi) = semi {
                let ent = &s[i..i + semi + 1];
                let decoded = match ent {
                    "&lt;" | "&LT;" => Some('<'),
                    "&gt;" | "&GT;" => Some('>'),
                    "&amp;" | "&AMP;" => Some('&'),
                    "&quot;" => Some('"'),
                    "&apos;" | "&#39;" | "&#x27;" | "&#X27;" => Some('\''),
                    // Non-breaking space: without this the serializer
                    // re-escapes the `&` and recipients literally read
                    // "&nbsp;" (`&amp;nbsp;` on the wire).
                    "&nbsp;" | "&NBSP;" => Some('\u{a0}'),
                    // Invisible format chars (newsletter spacer hacks):
                    // unknown named entities survive decoding, so the
                    // serializer re-escapes the `&` and readers see a
                    // literal "&zwnj;" instead of nothing.
                    "&zwnj;" | "&ZWNJ;" => Some('\u{200c}'),
                    "&zwj;" | "&ZWJ;" => Some('\u{200d}'),
                    "&lrm;" | "&LRM;" => Some('\u{200e}'),
                    "&rlm;" | "&RLM;" => Some('\u{200f}'),
                    "&shy;" | "&SHY;" => Some('\u{ad}'),
                    "&#34;" | "&#x22;" | "&#X22;" => Some('"'),
                    "&#60;" | "&#x3C;" | "&#x3c;" => Some('<'),
                    "&#62;" | "&#x3E;" | "&#x3e;" => Some('>'),
                    "&#38;" | "&#x26;" => Some('&'),
                    _ => super::entity_table::named(&ent[1..ent.len() - 1]),
                };
                if let Some(c) = decoded {
                    out.push(c);
                    i += semi + 1;
                    continue;
                }
                // Numeric entity fallback.
                if let Some(num) = ent.strip_prefix("&#") {
                    let num = &num[..num.len() - 1];
                    let val = if let Some(hex) =
                        num.strip_prefix('x').or_else(|| num.strip_prefix('X'))
                    {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        num.parse::<u32>().ok()
                    };
                    if let Some(v) = val.and_then(char::from_u32) {
                        // Refuse controls; keep text safe.
                        if !v.is_control() || v == '\n' || v == '\t' {
                            out.push(v);
                            i += semi + 1;
                            continue;
                        }
                    }
                }
                out.push('&');
                i += 1;
            } else {
                out.push('&');
                i += 1;
            }
        } else {
            // Push whole char (handles UTF-8).
            let ch = s[i..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}
