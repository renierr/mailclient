//! Safe HTML for mail: sanitize untrusted bodies (reader) and outgoing
//! composer HTML (send path) with std only (no new deps per AGENT.md).
//!
//! Mirrors the omamail idea (`tokenize → clean → serialise`, no regex):
//! where a tag ends must not be guessed — quoted `>` inside attributes
//! must not end the tag.
//!
//! Reader contract: [`sanitize`] strips scripts/styles/forms/active content,
//! `on*` handlers, `style` attributes (CSS `url()` tracking), dangerous
//! URLs (`javascript:`/`data:`-except-images/`file:`/...), and — unless
//! `allow_remote` — remote `<img src>` (records `had_remote` so QML can
//! offer "show once"). Text nodes are escaped on output.

/// Max input bytes examined (DoS cap for the GUI thread).
pub const MAX_HTML_BYTES: usize = 512_000;
/// Max output bytes emitted.
pub const MAX_OUT_BYTES: usize = 768_000;

/// Result of sanitizing one body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sanitized {
    pub html: String,
    /// True when a remote image was stripped/blocked (QML banner).
    pub had_remote: bool,
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

fn allowed_tag(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "p"
            | "br"
            | "div"
            | "span"
            | "b"
            | "strong"
            | "i"
            | "em"
            | "u"
            | "s"
            | "strike"
            | "blockquote"
            | "pre"
            | "code"
            | "ul"
            | "ol"
            | "li"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "tr"
            | "td"
            | "th"
            | "img"
    )
}

fn void_tag(tag: &str) -> bool {
    matches!(tag, "br" | "hr" | "img")
}

/// Tags whose *content* is dropped entirely (active/positional content).
fn drop_content_tag(tag: &str) -> bool {
    matches!(
        tag,
        "script"
            | "style"
            | "iframe"
            | "object"
            | "embed"
            | "form"
            | "input"
            | "button"
            | "select"
            | "textarea"
            | "meta"
            | "link"
            | "base"
            | "title"
            | "head"
            | "html"
            | "body"
            | "noscript"
            | "template"
            | "slot"
    )
}

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

fn escape_attr(s: &str) -> String {
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

fn scheme_of(url: &str) -> &str {
    let t = url.trim();
    match t.find(':') {
        Some(i) => &t[..i],
        None => "",
    }
}

fn is_http_url(url: &str) -> bool {
    let s = scheme_of(url).to_ascii_lowercase();
    s == "http" || s == "https"
}

fn is_public_remote(src: &str) -> bool {
    // Sender-controlled fetch gate (names only; connection pinning happens
    // in the WebEngine `autoLoadImages=false` default + no-redirect stance).
    // Refuse loopback/private/local/file outright — mirrors omamail policy.
    let t = urldecode_trim(src);
    let low = t.to_ascii_lowercase();
    if low.starts_with("cid:") || low.starts_with("data:image/") {
        return false;
    }
    if !(low.starts_with("http://") || low.starts_with("https://")) {
        return false;
    }
    let host = low
        .split_once("://")
        .map(|(_, r)| {
            r.split('/')
                .next()
                .unwrap_or("")
                .split('@')
                .next_back()
                .unwrap_or("")
        })
        .unwrap_or("");
    let host = host.split(':').next().unwrap_or("");
    if host.is_empty()
        || host == "localhost"
        || host.starts_with("127.")
        || host == "[::1]"
        || host == "::1"
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host.ends_with(".local")
        || host.contains("localhost")
    {
        return false;
    }
    if host.starts_with("172.") {
        let parts: Vec<&str> = host.split('.').collect();
        if parts.len() == 4 {
            if let Ok(n) = parts[1].parse::<u8>() {
                if (16..=31).contains(&n) {
                    return false;
                }
            }
        }
    }
    true
}

fn urldecode_trim(s: &str) -> String {
    decode_entities(s).trim().to_string()
}

fn safe_href(href: &str) -> Option<String> {
    let d = urldecode_trim(href);
    if d.is_empty() || d.starts_with('#') {
        return None;
    }
    let low = d.to_ascii_lowercase();
    if low.starts_with("http://") || low.starts_with("https://") || low.starts_with("mailto:") {
        // Reject embedded controls / quotes already handled by attr escaping;
        // reject `javascript:` smuggled after whitespace/entities (decoded above).
        Some(d)
    } else {
        None
    }
}

fn safe_img_src(src: &str, allow_remote: bool) -> Option<String> {
    let d = urldecode_trim(src);
    if d.is_empty() {
        return None;
    }
    let low = d.to_ascii_lowercase();
    if low.starts_with("cid:") {
        return Some(d);
    }
    if low.starts_with("data:image/png")
        || low.starts_with("data:image/jpeg")
        || low.starts_with("data:image/jpg")
        || low.starts_with("data:image/gif")
        || low.starts_with("data:image/webp")
    {
        if d.len() < 400_000 {
            return Some(d);
        }
        return None;
    }
    if is_http_url(&d) {
        if allow_remote && is_public_remote(&d) {
            return Some(d);
        }
        return None;
    }
    None
}

#[derive(Debug)]
struct Tag {
    name: String,
    attrs: Vec<(String, String)>,
    closing: bool,
    self_closing: bool,
}

/// Parse one `<...>` starting at `bytes[start] == b'<'`.
/// Returns (tag, next_index). Correctly skips `>` inside quotes.
fn parse_tag(bytes: &[u8], start: usize) -> (Option<Tag>, usize) {
    let i = start + 1;
    // Comments / doctype / processing instructions: drop whole.
    if i < bytes.len() && bytes[i] == b'!' {
        // <!-- ... -->
        if bytes.get(i + 1) == Some(&b'-') && bytes.get(i + 2) == Some(&b'-') {
            if let Some(end) = find_sub(bytes, b"-->", i + 3) {
                return (None, end + 3);
            }
            return (None, bytes.len());
        }
        if let Some(end) = find_byte(bytes, b'>', i) {
            return (None, end + 1);
        }
        return (None, bytes.len());
    }
    if i < bytes.len() && bytes[i] == b'?' {
        if let Some(end) = find_sub(bytes, b"?>", i) {
            return (None, end + 2);
        }
        return (None, bytes.len());
    }
    // Find matching `>` respecting quotes.
    let mut j = i;
    let mut quote = 0u8;
    while j < bytes.len() {
        let c = bytes[j];
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
        } else if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'>' {
            break;
        }
        j += 1;
    }
    if j >= bytes.len() {
        return (None, bytes.len());
    }
    let inner = String::from_utf8_lossy(&bytes[i..j]).to_string();
    let next = j + 1;
    let t = inner.trim();
    if t.is_empty() {
        return (None, next);
    }
    let closing = t.starts_with('/');
    let body = if closing { t[1..].trim() } else { t };
    let self_closing = body.ends_with('/');
    let body = if self_closing {
        body[..body.len() - 1].trim()
    } else {
        body
    };
    // Name = up to ws or '/'.
    let mut split = body.len();
    for (k, c) in body.char_indices() {
        if c.is_whitespace() || c == '/' {
            split = k;
            break;
        }
    }
    let name = body[..split].to_ascii_lowercase();
    if name.is_empty() || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        return (None, next);
    }
    let attrs = parse_attrs(&body[split..]);
    (
        Some(Tag {
            name,
            attrs,
            closing,
            self_closing,
        }),
        next,
    )
}

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (is_ws(b[i]) || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() {
            break;
        }
        let ns = i;
        while i < b.len() && !is_ws(b[i]) && b[i] != b'=' && b[i] != b'/' && b[i] != b'>' {
            i += 1;
        }
        if ns == i {
            i += 1;
            continue;
        }
        let name = s[ns..i].to_ascii_lowercase();
        while i < b.len() && is_ws(b[i]) {
            i += 1;
        }
        let mut val = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && is_ws(b[i]) {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                val = s[vs..i.min(b.len())].to_string();
                i = (i + 1).min(b.len());
            } else {
                let vs = i;
                while i < b.len() && !is_ws(b[i]) && b[i] != b'>' {
                    i += 1;
                }
                val = s[vs..i].to_string();
            }
        }
        if !name.is_empty() {
            out.push((name, val));
        }
        if out.len() > 32 {
            break;
        }
    }
    out
}

fn find_byte(h: &[u8], n: u8, from: usize) -> Option<usize> {
    h.iter().skip(from).position(|c| *c == n).map(|p| p + from)
}

/// Parse a small clamped uint (colspan/rowspan); `None` on junk.
fn parse_small_uint(v: &str, lo: u32, hi: u32) -> Option<u32> {
    if v.len() >= 4 || !v.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    v.parse::<u32>().ok().map(|n| n.clamp(lo, hi))
}

fn find_sub(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    h.windows(n.len())
        .skip(from)
        .position(|w| w == n)
        .map(|p| p + from)
}

/// Sanitize untrusted HTML. `allow_remote=false` (reader default) strips
/// remote `<img>` and records `had_remote`.
pub fn sanitize(raw: &str, allow_remote: bool) -> Sanitized {
    let raw = if raw.len() > MAX_HTML_BYTES {
        &raw[..MAX_HTML_BYTES]
    } else {
        raw
    };
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

fn push_capped(out: &mut String, s: &str) {
    if out.len() + s.len() > MAX_OUT_BYTES + 1024 {
        return;
    }
    out.push_str(s);
}

/// Outgoing composer HTML: same gate, but the user *meant* remote images.
pub fn sanitize_for_send(raw: &str) -> String {
    sanitize(raw, true).html
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_and_handlers() {
        let s = sanitize("<p onclick=\"x()\">hi<script>alert(1)</script></p>", false);
        assert!(!s.html.contains("script"));
        assert!(!s.html.contains("onclick"));
        assert!(s.html.contains("hi"));
    }

    #[test]
    fn quoted_gt_does_not_end_tag() {
        let s = sanitize("<img alt=\"a>b\" src=\"cid:x\">", false);
        assert!(s.html.contains("cid:x"));
        assert!(!s.html.contains("a&gt;b\">"));
    }

    #[test]
    fn blocks_remote_img_by_default() {
        let s = sanitize("<img src=\"https://example.com/t.png\" alt=\"t\">", false);
        assert!(s.had_remote);
        assert!(!s.html.contains("example.com"));
        let s2 = sanitize("<img src=\"https://example.com/t.png\">", true);
        assert!(s2.html.contains("example.com"));
    }

    #[test]
    fn blocks_private_hosts_even_when_allowed() {
        let s = sanitize("<img src=\"http://192.168.1.2/x.png\">", true);
        assert!(!s.html.contains("192.168"));
    }

    #[test]
    fn rejects_javascript_href() {
        let s = sanitize("<a href=\"javascript:alert(1)\">x</a>", true);
        assert!(!s.html.contains("javascript"));
        assert!(s.html.contains("x"));
        let ok = sanitize("<a href=\"https://example.com\">x</a>", true);
        assert!(ok.html.contains("https://example.com"));
    }

    #[test]
    fn style_attr_dropped() {
        let s = sanitize(
            "<p style=\"background:url(https://example.com/x)\">t</p>",
            true,
        );
        assert!(!s.html.contains("example.com"));
        assert!(s.html.contains("t"));
    }

    #[test]
    fn html_to_text_keeps_lines() {
        assert_eq!(html_to_text("<p>hi<br>there</p>"), "hi\nthere");
    }

    #[test]
    fn looks_like_html_ignores_stray() {
        assert!(!looks_like_html("I <3 you 5 > 3"));
        assert!(looks_like_html("<p>hi</p>"));
    }
}
