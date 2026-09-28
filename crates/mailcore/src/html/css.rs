//! Inline `style` and presentational attributes that may survive.
//!
//! Newsletters are laid out with inline CSS and table attributes; dropping
//! them all leaves a grey column of text. What is kept is presentation only:
//! an allow-list of properties, and no value that can fetch (`url()`,
//! `@import`, `image-set`), run (`expression`, `behavior`, `-moz-binding`)
//! or hide either of those behind CSS escapes or comments. Positioning is
//! left out, so a mail cannot paint over the reader's own chrome.

use super::entities::decode_entities;

/// Longest `style` value examined, and longest one emitted.
const MAX_STYLE_IN: usize = 4_000;
const MAX_STYLE_OUT: usize = 1_500;

fn allowed_property(p: &str) -> bool {
    matches!(
        p,
        "color"
            | "background"
            | "background-color"
            | "font"
            | "font-family"
            | "font-size"
            | "font-style"
            | "font-weight"
            | "font-variant"
            | "text-align"
            | "text-decoration"
            | "text-indent"
            | "text-transform"
            | "line-height"
            | "letter-spacing"
            | "word-spacing"
            | "white-space"
            | "word-break"
            | "word-wrap"
            | "overflow-wrap"
            | "vertical-align"
            | "direction"
            | "width"
            | "min-width"
            | "max-width"
            | "height"
            | "min-height"
            | "max-height"
            | "margin"
            | "margin-top"
            | "margin-right"
            | "margin-bottom"
            | "margin-left"
            | "padding"
            | "padding-top"
            | "padding-right"
            | "padding-bottom"
            | "padding-left"
            | "border"
            | "border-top"
            | "border-right"
            | "border-bottom"
            | "border-left"
            | "border-color"
            | "border-style"
            | "border-width"
            | "border-radius"
            | "border-collapse"
            | "border-spacing"
            | "table-layout"
            | "list-style-type"
            | "display"
            | "float"
            | "clear"
            | "opacity"
            | "overflow"
            | "mso-hide"
    )
}

/// `display` values that stay in normal flow. `none` is kept on purpose:
/// newsletters hide their preheader text with it.
fn allowed_display(v: &str) -> bool {
    matches!(
        v,
        "block"
            | "inline"
            | "inline-block"
            | "none"
            | "table"
            | "table-row"
            | "table-cell"
            | "list-item"
    )
}

fn safe_value(v: &str) -> bool {
    let low = v.to_ascii_lowercase();
    // Backslash escapes and comments can spell any of the words below.
    !v.contains(['\\', '<', '>', '@', '{', '}'])
        && !low.contains("/*")
        && !low.contains("url")
        && !low.contains("image")
        && !low.contains("expression")
        && !low.contains("javascript")
        && !low.contains("behavior")
        && !low.contains("binding")
        && !low.contains("var(")
        && !low.contains("attr(")
}

/// Filter one `style` attribute value down to allowed declarations.
/// `None` when nothing survives.
pub(super) fn sanitize_style(raw: &str) -> Option<String> {
    let decoded = decode_entities(raw);
    if decoded.len() > MAX_STYLE_IN {
        return None;
    }
    let mut out = String::new();
    for decl in decoded.split(';') {
        let Some((prop, value)) = decl.split_once(':') else {
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty() || !allowed_property(&prop) || !safe_value(value) {
            continue;
        }
        if prop == "display" && !allowed_display(&value.to_ascii_lowercase()) {
            continue;
        }
        let piece = format!("{prop}:{value};");
        if out.len() + piece.len() > MAX_STYLE_OUT {
            break;
        }
        out.push_str(&piece);
    }
    (!out.is_empty()).then_some(out)
}

/// Whether sanitized HTML paints its own colours (text, backgrounds).
///
/// Readers use it to pick a rendering: a mail without any can take the app
/// theme as it is; a designed one needs its colours kept or darkened as a
/// whole, since theming only half of it leaves dark text on dark ground.
/// Works on [`super::sanitize`] output, where declarations are normalised
/// to `prop:value;` and attributes to `name="…"`.
pub fn has_own_colors(sanitized: &str) -> bool {
    // Only markup counts: text is escaped, so `<` always opens a tag and
    // "color:" in a sentence is not a colour.
    sanitized.split('<').skip(1).any(|chunk| {
        let tag = chunk.split('>').next().unwrap_or("").to_ascii_lowercase();
        ["color:", "background:", " bgcolor=\"", " color=\""]
            .iter()
            .any(|p| tag.contains(p))
    })
}

/// `bgcolor` / `color`: `#rgb`, `#rrggbb` or a plain colour name.
pub(super) fn safe_color(raw: &str) -> Option<String> {
    let v = raw.trim();
    let ok = match v.strip_prefix('#') {
        Some(hex) => matches!(hex.len(), 3 | 6) && hex.bytes().all(|c| c.is_ascii_hexdigit()),
        None => !v.is_empty() && v.len() <= 20 && v.bytes().all(|c| c.is_ascii_alphabetic()),
    };
    ok.then(|| v.to_string())
}

/// Pixel or percentage size for `width` / `height`, clamped.
pub(super) fn safe_length(raw: &str) -> Option<String> {
    let v = raw.trim();
    if let Some(p) = v.strip_suffix('%') {
        let n: u32 = p.trim().parse().ok()?;
        return Some(format!("{}%", n.min(100)));
    }
    let d = v.trim_end_matches("px");
    if d.is_empty() || d.len() >= 8 || !d.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    d.parse::<u32>().ok().map(|n| n.min(1200).to_string())
}

/// Keyword attributes (`align`, `valign`, `dir`): one of `allowed`.
pub(super) fn keyword<'a>(raw: &str, allowed: &[&'a str]) -> Option<&'a str> {
    let v = raw.trim().to_ascii_lowercase();
    allowed.iter().copied().find(|a| *a == v)
}

/// `face` on `<font>`: a font list, letters, digits, spaces, commas, dashes
/// and quotes only.
pub(super) fn safe_font_face(raw: &str) -> Option<String> {
    let v = raw.trim();
    let ok = !v.is_empty()
        && v.len() <= 100
        && v.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | ',' | '-' | '\'' | '"'));
    ok.then(|| v.to_string())
}
