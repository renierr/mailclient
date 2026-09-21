//! Tokenizing: which tags are allowed, and where one ends.
//!
//! `>` inside a quoted attribute does not end a tag, so the end is parsed
//! rather than searched for.

pub(super) fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0C)
}

pub(super) fn allowed_tag(tag: &str) -> bool {
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

pub(super) fn void_tag(tag: &str) -> bool {
    matches!(tag, "br" | "hr" | "img")
}

/// Tags whose *content* is dropped entirely (active/positional content).
/// Tags whose **content** is dropped along with the tag.
///
/// `html` and `body` must NOT be listed here, and neither may the void
/// `meta`/`link`/`base`: dropping their content means dropping the whole
/// document. A full document is the normal case -- it is what Qt's rich-text
/// editor emits and what most HTML mail looks like -- and listing them here
/// sanitized every such body down to an empty string (an unsent body, and an
/// empty reader pane). They fall through to `allowed_tag` instead, which
/// skips the tag and keeps what is inside it.
///
/// `head` stays, so the `<style>`/`<meta>` block it wraps still goes away.
pub(super) fn drop_content_tag(tag: &str) -> bool {
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
            | "title"
            | "head"
            | "noscript"
            | "template"
            | "slot"
    )
}

#[derive(Debug)]
pub(super) struct Tag {
    pub(super) name: String,
    pub(super) attrs: Vec<(String, String)>,
    pub(super) closing: bool,
    pub(super) self_closing: bool,
}

/// Parse one `<...>` starting at `bytes[start] == b'<'`.
/// Returns (tag, next_index). Correctly skips `>` inside quotes.
pub(super) fn parse_tag(bytes: &[u8], start: usize) -> (Option<Tag>, usize) {
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

pub(super) fn parse_attrs(s: &str) -> Vec<(String, String)> {
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

pub(super) fn find_byte(h: &[u8], n: u8, from: usize) -> Option<usize> {
    h.iter().skip(from).position(|c| *c == n).map(|p| p + from)
}

/// Parse a small clamped uint (colspan/rowspan); `None` on junk.
pub(super) fn parse_small_uint(v: &str, lo: u32, hi: u32) -> Option<u32> {
    if v.len() >= 4 || !v.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    v.parse::<u32>().ok().map(|n| n.clamp(lo, hi))
}

pub(super) fn find_sub(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
    h.windows(n.len())
        .skip(from)
        .position(|w| w == n)
        .map(|p| p + from)
}
