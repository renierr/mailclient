//! Entity decoding and escaping.
//!
//! Only the subset that changes a safety decision or would otherwise show
//! up as literal `&nbsp;` text — not a full HTML5 entity table.

/// Decode the small entity subset needed for URL decisions + text output.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'&' {
            if let Some(semi) = s[i..].find(';').filter(|n| *n < 24) {
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
                    _ => None,
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

pub(super) fn escape_attr(s: &str) -> String {
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
