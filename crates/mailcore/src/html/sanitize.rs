//! The sanitizer itself: walk the input, keep what is allowed, serialise.

use super::css::{keyword, safe_color, safe_font_face, safe_length, sanitize_style};
use super::entities::{decode_entities, escape_attr, escape_text};
use super::tags::{
    allowed_tag, drop_content_name, head_child_tag, is_table_tag, parse_small_uint, parse_tag,
    void_tag,
};
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

/// Nesting bound for dropped content: past it, inner opens are not
/// tracked, so the stack stays small on hostile input.
const MAX_DROP_NESTING: usize = 256;

/// Whether this tag ends an open `<head>`: `</head>`, `</body>`, `</html>`,
/// `</br>`, or any start tag that cannot live in a head (`<body>`, `<p>`).
/// Most mail never writes `</head>`; without this an unclosed head hid the
/// whole message (C6).
fn ends_head(name: &str, closing: bool) -> bool {
    if closing {
        matches!(name, "head" | "body" | "html" | "br")
    } else {
        !head_child_tag(name)
    }
}

/// Sanitize untrusted HTML. `allow_remote=false` (reader default) strips
/// remote `<img>` and records `had_remote`.
pub fn sanitize(raw: &str, allow_remote: bool) -> Sanitized {
    let raw = truncate_on_char_boundary(raw, MAX_HTML_BYTES);
    let bytes = raw.as_bytes();
    let mut out = String::new();
    let mut had_remote = false;
    let mut i = 0;
    // Open drop-content tags, innermost last. A stack rather than a depth
    // count: a close only ends its own name (a stray `</form>` inside a
    // `<style>` must not end it), and an unclosed `<head>` can be ended
    // the way a parser ends it instead of swallowing the whole body.
    let mut dropping: Vec<&'static str> = Vec::new();
    let mut open: Vec<String> = Vec::new();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let (tag, next) = parse_tag(bytes, i);
            i = next;
            let Some(t) = tag else { continue };
            if dropping.last() == Some(&"head") && ends_head(&t.name, t.closing) {
                dropping.pop();
            }
            if !dropping.is_empty() {
                if let Some(name) = drop_content_name(&t.name) {
                    if t.closing {
                        if let Some(pos) = dropping.iter().rposition(|n| *n == name) {
                            dropping.truncate(pos);
                        }
                    } else if !t.self_closing && dropping.len() < MAX_DROP_NESTING {
                        dropping.push(name);
                    }
                }
                continue;
            }
            if let Some(name) = drop_content_name(&t.name) {
                if !t.closing && !t.self_closing {
                    dropping.push(name);
                }
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
                if k.starts_with("on") || k == "class" || k == "id" {
                    continue;
                }
                if let Some(kept) = presentational(&t.name, k, v) {
                    attrs_out.push_str(&format!(" {k}=\"{}\"", escape_attr(&kept)));
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
                    _ => {}
                }
            }
            if t.name == "img" && !attrs_out.contains("src=") {
                // Blocked remote image: our own compact badge instead of the
                // raw alt text. Inline `[image: {full alt}]` broke narrow
                // table cells (`td{overflow-wrap:anywhere}` wraps it
                // per-character into tall coloured columns, e.g. shipment
                // trackers). The disclosure below stays 64x48; hover
                // (`title`) and tap (`details`, no script) reveal the
                // original alt. If alt present, surface it so newsletters
                // don't go blank.
                if let Some((_, alt)) = t.attrs.iter().find(|(k, _)| k == "alt") {
                    if !alt.trim().is_empty() {
                        push_capped(&mut out, &blocked_img_placeholder(alt));
                    }
                } else if had_remote {
                    push_capped(&mut out, &blocked_img_placeholder(""));
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
            if dropping.last() == Some(&"head") && !chunk.trim().is_empty() {
                dropping.pop();
            }
            if dropping.is_empty() {
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

/// Placeholder for a blocked or missing `<img>`: our own small badge.
///
/// A `<details class="mc-blocked">` disclosure holding an inline SVG
/// pictogram (mountain + sun, `#888` reads on light and dark sheets).
/// Collapsed it is always 64x48, so narrow table cells keep their layout.
/// Hover shows the original alt via `title`; tapping the badge expands the
/// `<span>` with the full alt text. HTML + CSS only — no script, and the
/// inline `<svg>` carries none. With no alt the label reads
/// `Blocked image`.
pub(crate) fn blocked_img_placeholder(alt: &str) -> String {
    let title: String = alt.trim().chars().take(200).collect();
    let label = if title.is_empty() {
        "Blocked image".to_string()
    } else {
        title.clone()
    };
    format!(
        "<details class=\"mc-blocked\"><summary title=\"{}\">{SVG_BADGE}</summary><span>{}</span></details>",
        escape_attr(&label),
        escape_text(&label)
    )
}

/// The badge: dashed rounded frame, sun, mountain. Static shapes only —
/// no `script`, no handlers, no external references.
const SVG_BADGE: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"48\" viewBox=\"0 0 64 48\"><rect x=\"1\" y=\"1\" width=\"62\" height=\"46\" rx=\"6\" fill=\"none\" stroke=\"#888888\" stroke-width=\"1.5\" stroke-dasharray=\"5 3\"/><circle cx=\"22\" cy=\"18\" r=\"4\" fill=\"none\" stroke=\"#888888\" stroke-width=\"1.5\"/><path d=\"M12 36 L26 24 L34 31 L40 26 L52 36\" fill=\"none\" stroke=\"#888888\" stroke-width=\"1.5\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/></svg>";

/// Layout attributes that are safe on any allowed tag: `style` filtered to
/// presentation, colours, sizes, alignment and table spacing. Everything
/// that can name a URL is handled by the caller instead.
fn presentational(tag: &str, k: &str, v: &str) -> Option<String> {
    let table = is_table_tag(tag);
    match k {
        "style" => sanitize_style(v),
        "dir" => keyword(v, &["ltr", "rtl", "auto"]).map(str::to_string),
        "align" => keyword(v, &["left", "center", "right", "justify"]).map(str::to_string),
        "valign" if table => {
            keyword(v, &["top", "middle", "bottom", "baseline"]).map(str::to_string)
        }
        "bgcolor" if table => safe_color(v),
        "color" if tag == "font" => safe_color(v),
        "face" if tag == "font" => safe_font_face(v),
        "size" if tag == "font" => parse_small_uint(v.trim(), 1, 7).map(|n| n.to_string()),
        "width" | "height" if tag == "img" || tag == "col" || table => safe_length(v),
        "border" | "cellpadding" | "cellspacing" if tag == "table" => {
            parse_small_uint(v.trim(), 0, 40).map(|n| n.to_string())
        }
        "colspan" | "rowspan" if tag == "td" || tag == "th" => {
            parse_small_uint(v.trim(), 1, 20).map(|n| n.to_string())
        }
        "span" if tag == "col" || tag == "colgroup" => {
            parse_small_uint(v.trim(), 1, 20).map(|n| n.to_string())
        }
        _ => None,
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
