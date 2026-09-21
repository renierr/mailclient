//! The other directions: HTML to plain text, plain text to HTML, and the
//! "is this really formatted?" heuristics the send path asks about.

use super::entities::{decode_entities, escape_text};
use super::tags::{drop_content_tag, parse_tag};
use super::MAX_OUT_BYTES;

/// Heuristic (Rust side, no QML guessing): real tags, not stray `<3`.
pub fn looks_like_html(s: &str) -> bool {
    let low = s.to_ascii_lowercase();
    for tag in [
        "<p",
        "<div",
        "<br",
        "<a ",
        "<a>",
        "<b>",
        "<b ",
        "<i>",
        "<table",
        "<img",
        "<html",
        "<body",
        "<ul",
        "<ol",
        "<li",
        "<h1",
        "<h2",
        "<h3",
        "<blockquote",
    ] {
        if low.contains(tag) {
            return true;
        }
    }
    false
}

/// Whether sanitized composer HTML carries formatting that plain text cannot
/// express — the Auto send-format signal. Plain structure (`p`, `div`, `br`,
/// bare `span`, document wrappers) does NOT count: the WYSIWYG editor emits
/// those even for unformatted typing, and they round-trip through
/// `html_to_text` losslessly.
#[must_use]
pub fn needs_html_formatting(html: &str) -> bool {
    // Formatting-bearing tags. `span` only counts with attributes (a bare
    // `<span>` carries no styling); every other tag here always formats.
    const TAGS: &[&str] = &[
        "b",
        "strong",
        "i",
        "em",
        "u",
        "ins",
        "s",
        "strike",
        "del",
        "a",
        "ul",
        "ol",
        "li",
        "blockquote",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "img",
        "table",
        "pre",
        "code",
        "sub",
        "sup",
        "font",
        "hr",
        "dl",
        "dt",
        "dd",
        "span",
    ];
    let low = html.to_ascii_lowercase();
    let bytes = low.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if j < bytes.len() && bytes[j] == b'/' {
            j += 1;
        }
        let start = j;
        while j < bytes.len() && (bytes[j].is_ascii_alphanumeric()) {
            j += 1;
        }
        let name = &low[start..j];
        // Tag boundary: `<b>`, `<b …>`, `<b/>` — but not `<blockquote` when
        // checking `b`, since the name scan already consumed the full name.
        let boundary = j < bytes.len()
            && (bytes[j] == b'>' || bytes[j] == b'/' || bytes[j].is_ascii_whitespace());
        if boundary && TAGS.contains(&name) {
            // Bare `<span>` is structural noise; styled spans format.
            if name != "span" || (j < bytes.len() && bytes[j] != b'>') {
                return true;
            }
        }
        i = j.max(i + 1);
    }
    false
}

/// Strip tags → plain text (reply quotes, plain fallback).
pub fn html_to_text(html: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut drop_depth = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let (tag, next) = parse_tag(bytes, i);
            i = next;
            let Some(t) = tag else { continue };
            if !t.closing
                && drop_content_tag(&t.name)
                && !t.self_closing
                && t.name != "head"
                && t.name != "html"
                && t.name != "body"
            {
                if t.name == "script" || t.name == "style" {
                    drop_depth += 1;
                }
                continue;
            }
            if t.closing && (t.name == "script" || t.name == "style") {
                drop_depth = drop_depth.saturating_sub(1);
                continue;
            }
            if drop_depth > 0 {
                continue;
            }
            if !t.closing {
                match t.name.as_str() {
                    "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                    | "blockquote" => {
                        out.push('\n');
                    }
                    _ => {}
                }
            } else {
                match t.name.as_str() {
                    "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "blockquote" => out.push('\n'),
                    _ => {}
                }
            }
        } else {
            let ns = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            if drop_depth == 0 {
                out.push_str(&decode_entities(&String::from_utf8_lossy(&bytes[ns..i])));
            }
        }
        if out.len() > MAX_OUT_BYTES {
            break;
        }
    }
    // Collapse 3+ newlines, trim trailing spaces per line.
    let mut lines = Vec::new();
    for l in out.split('\n') {
        lines.push(l.trim_end());
        if lines.len() > 4000 {
            break;
        }
    }
    let mut collapsed = String::new();
    let mut blanks = 0;
    for l in lines {
        if l.trim().is_empty() {
            blanks += 1;
            if blanks <= 1 {
                collapsed.push('\n');
            }
        } else {
            blanks = 0;
            collapsed.push_str(l);
            collapsed.push('\n');
        }
    }
    collapsed.trim().to_string()
}

/// Plain → minimal safe HTML (used when composer sends text but format
/// demands HTML).
pub fn text_to_html(text: &str) -> String {
    let esc = escape_text(text);
    let mut out = String::from("<p>");
    for (idx, para) in esc.split("\n\n").enumerate() {
        if idx > 0 {
            out.push_str("</p><p>");
        }
        out.push_str(&para.replace('\n', "<br>"));
        if out.len() > MAX_OUT_BYTES {
            break;
        }
    }
    out.push_str("</p>");
    out
}

/// Wrap sanitized inner HTML in a small readable document (added *after*
/// sanitizing so the wrapper is trusted).
pub fn wrap_document(inner_sanitized: &str) -> String {
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
        <style>body{{font-family:sans-serif;font-size:14px;line-height:1.5;max-width:72ch;margin:12px;word-wrap:break-word}}\
        img{{max-width:100%;height:auto}}pre{{white-space:pre-wrap}}table{{border-collapse:collapse}}td,th{{padding:4px 8px}}</style>\
        </head><body>{inner_sanitized}</body></html>"
    )
}
