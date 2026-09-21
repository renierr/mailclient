//! The sanitizer itself: walk the input, keep what is allowed, serialise.

use super::entities::{decode_entities, escape_attr, escape_text};
use super::tags::{allowed_tag, drop_content_tag, parse_small_uint, parse_tag, void_tag};
use super::urls::{is_http_url, safe_href, safe_img_src, urldecode_trim};
use super::{Sanitized, MAX_HTML_BYTES, MAX_OUT_BYTES};

/// Truncate to at most `max` bytes without splitting a character.
///
/// `&s[..max]` panics when `max` lands inside a multi-byte sequence, which
/// one oversized non-ASCII mail is enough to hit. UTF-8 continuation bytes
/// are `10xxxxxx`, so walking back to the first non-continuation byte finds
/// the boundary; at most three steps.
pub(super) fn truncate_on_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let bytes = s.as_bytes();
    let mut end = max;
    while end > 0 && (bytes[end] & 0b1100_0000) == 0b1000_0000 {
        end -= 1;
    }
    &s[..end]
}

/// Sanitize untrusted HTML. `allow_remote=false` (reader default) strips
/// remote `<img>` and records `had_remote`.
pub fn sanitize(raw: &str, allow_remote: bool) -> Sanitized {
    let raw = truncate_on_char_boundary(raw, MAX_HTML_BYTES);
    let bytes = raw.as_bytes();
    let mut out = String::new();
    let mut had_remote = false;
    let mut i = 0;
    let mut drop_depth: usize = 0;
    let mut open: Vec<String> = Vec::new();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let (tag, next) = parse_tag(bytes, i);
            i = next;
            let Some(t) = tag else { continue };
            if drop_depth > 0 {
                if t.closing && drop_content_tag(&t.name) {
                    drop_depth = drop_depth.saturating_sub(1);
                } else if !t.closing && drop_content_tag(&t.name) && !t.self_closing {
                    drop_depth += 1;
                }
                continue;
            }
            if !t.closing && drop_content_tag(&t.name) && !t.self_closing {
                drop_depth += 1;
                continue;
            }
            if drop_content_tag(&t.name) {
                continue;
            }
            if !allowed_tag(&t.name) {
                continue;
            }
            if t.closing {
                if void_tag(&t.name) {
                    continue;
                }
                // Close matching open tag (pop mismatches, stay well-formed).
                if let Some(pos) = open.iter().rposition(|o| *o == t.name) {
                    while open.len() > pos {
                        let o = open.pop().unwrap_or_default();
                        push_capped(&mut out, &format!("</{o}>"));
                    }
                }
                continue;
            }
            // Build allow-listed attributes.
            let mut attrs_out = String::new();
            for (k, v) in &t.attrs {
                if k.starts_with("on") || k == "style" || k == "class" || k == "id" {
                    continue;
                }
                match t.name.as_str() {
                    "a" if k == "href" => {
                        if let Some(h) = safe_href(v) {
                            attrs_out.push_str(&format!(
                                " href=\"{}\" rel=\"noopener\"",
                                escape_attr(&h)
                            ));
                        }
                    }
                    "img" if k == "src" => match safe_img_src(v, allow_remote) {
                        Some(s) => attrs_out.push_str(&format!(" src=\"{}\"", escape_attr(&s))),
                        None => {
                            if is_http_url(&urldecode_trim(v)) {
                                had_remote = true;
                            }
                        }
                    },
                    "img" if k == "alt" => {
                        let vv: String = v.chars().take(200).collect();
                        attrs_out.push_str(&format!(" alt=\"{}\"", escape_attr(&vv)));
                    }
                    "img" | "td" | "th" if k == "width" || k == "height" => {
                        let d = v.trim().trim_end_matches("px");
                        if !d.is_empty() && d.len() < 8 && d.bytes().all(|c| c.is_ascii_digit()) {
                            if let Ok(n) = d.parse::<u32>() {
                                let n = n.min(1200);
                                attrs_out.push_str(&format!(" {k}=\"{n}\""));
                            }
                        }
                    }
                    "td" | "th" if k == "colspan" || k == "rowspan" => {
                        if let Some(n) = parse_small_uint(v, 1, 20) {
                            attrs_out.push_str(&format!(" {k}=\"{n}\""));
                        }
                    }
                    _ => {}
                }
            }
            if t.name == "img" && !attrs_out.contains("src=") {
                // Keep alt text only: emit nothing (text already outside tag).
                // If alt present, surface it so newsletters don't go blank.
                if let Some((_, alt)) = t.attrs.iter().find(|(k, _)| k == "alt") {
                    let a: String = alt.chars().take(120).collect();
                    if !a.trim().is_empty() {
                        push_capped(&mut out, &escape_text(&format!("[image: {}]", a.trim())));
                    }
                } else if had_remote {
                    push_capped(&mut out, "[image blocked]");
                }
                continue;
            }
            if void_tag(&t.name) {
                push_capped(&mut out, &format!("<{}{}>", t.name, attrs_out));
            } else {
                push_capped(&mut out, &format!("<{}{}>", t.name, attrs_out));
                open.push(t.name.clone());
                if open.len() > 64 {
                    open.remove(0);
                }
            }
            if out.len() > MAX_OUT_BYTES {
                break;
            }
        } else {
            // Text run until next `<`.
            let ns = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            let chunk = String::from_utf8_lossy(&bytes[ns..i]).to_string();
            if drop_depth == 0 {
                push_capped(&mut out, &escape_text(&decode_entities(&chunk)));
            }
            if out.len() > MAX_OUT_BYTES {
                break;
            }
        }
    }
    for o in open.iter().rev() {
        if out.len() > MAX_OUT_BYTES {
            break;
        }
        push_capped(&mut out, &format!("</{o}>"));
    }
    Sanitized {
        html: out,
        had_remote,
    }
}

pub(super) fn push_capped(out: &mut String, s: &str) {
    if out.len() + s.len() > MAX_OUT_BYTES + 1024 {
        return;
    }
    out.push_str(s);
}

/// Outgoing composer HTML: same gate, but the user *meant* remote images.
pub fn sanitize_for_send(raw: &str) -> String {
    sanitize(raw, true).html
}
