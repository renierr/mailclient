//! The dark rewrite of a designed mail: the sender's colours, inverted once
//! at load with the same matrix a CSS `invert(1) hue-rotate(180deg)` filter
//! uses, so the page looks exactly as filtered without running one.
//!
//! Input is sanitizer output: no `<style>` element is left, so every author
//! colour sits in an inline `style` or a `color`/`bgcolor` attribute. Only
//! those change. Values that are not a plain colour (gradients, keywords
//! like `inherit`) are left alone.

use super::attrs::{find_attr, map_tags, splice};
use super::named_colors::NAMED;
use super::Rgb;

/// Properties whose value must be one colour to convert.
const FLAT: &[&str] = &[
    "color",
    "background-color",
    "background",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "border-color",
    "outline-color",
    "text-decoration-color",
    "fill",
    "stroke",
];

/// Shorthands that carry width and style next to the colour: the colour
/// token inside is replaced rather than the whole value.
const SHORTHAND: &[&str] = &[
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "outline",
];

/// A colour keyword's value; `transparent` and `currentcolor` are not
/// colours to invert.
fn named(word: &str) -> Option<u32> {
    let w = word.to_ascii_lowercase();
    NAMED
        .binary_search_by(|(k, _)| k.cmp(&w.as_str()))
        .ok()
        .map(|i| NAMED[i].1)
}

/// One CSS colour value as a colour and its alpha; `None` for anything
/// else (keywords like `inherit`, gradients).
fn parse_color(value: &str) -> Option<(Rgb, u8)> {
    let v = value.trim();
    if let Some(hex) = v.strip_prefix('#') {
        if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        return match hex.len() {
            3 | 6 => Rgb::parse(v).map(|c| (c, 255)),
            8 => {
                let rgb = u32::from_str_radix(&hex[..6], 16).ok()?;
                let a = u8::from_str_radix(&hex[6..], 16).ok()?;
                Some((Rgb(rgb), a))
            }
            _ => None,
        };
    }
    let low = v.to_ascii_lowercase();
    if let Some(args) = low
        .strip_prefix("rgba(")
        .or_else(|| low.strip_prefix("rgb("))
        .and_then(|r| r.strip_suffix(')'))
    {
        let parts: Vec<&str> = args.split(',').map(str::trim).collect();
        if !(3..=4).contains(&parts.len()) {
            return None;
        }
        let number = |s: &str| -> Option<(f64, bool)> {
            let (n, pct) = match s.strip_suffix('%') {
                Some(n) => (n, true),
                None => (s, false),
            };
            let ok = !n.is_empty()
                && !n.starts_with('.')
                && !n.ends_with('.')
                && n.bytes().all(|c| c.is_ascii_digit() || c == b'.')
                && n.bytes().filter(|&c| c == b'.').count() <= 1;
            ok.then(|| n.parse().ok().map(|v| (v, pct))).flatten()
        };
        let mut rgb = 0u32;
        for part in &parts[..3] {
            let (n, pct) = number(part)?;
            let byte = if pct { n * 255.0 / 100.0 } else { n };
            rgb = (rgb << 8) | byte.round().clamp(0.0, 255.0) as u32;
        }
        let alpha = match parts.get(3) {
            Some(a) => {
                let (n, pct) = number(a)?;
                let a = if pct { n * 255.0 / 100.0 } else { n * 255.0 };
                a.round().clamp(0.0, 255.0) as u8
            }
            None => 255,
        };
        return Some((Rgb(rgb), alpha));
    }
    named(v).map(|c| (Rgb(c), 255))
}

/// A colour back to CSS: opaque as `#rrggbb`, translucent as `rgba(...)`.
fn to_css(c: Rgb, alpha: u8) -> String {
    if alpha == 255 {
        return c.css();
    }
    let a = format!("{:.3}", f64::from(alpha) / 255.0);
    let mut a = a.trim_end_matches('0').to_string();
    if a.ends_with('.') {
        a.push('0');
    }
    format!(
        "rgba({},{},{},{a})",
        (c.0 >> 16) & 0xFF,
        (c.0 >> 8) & 0xFF,
        c.0 & 0xFF
    )
}

/// One colour value inverted, keeping a trailing `!important`; `None` when
/// it holds no convertible colour.
fn convert_value(value: &str) -> Option<String> {
    let trimmed = value.trim_end();
    let low = trimmed.to_ascii_lowercase();
    let (v, important) = match low.strip_suffix("!important") {
        Some(rest) => {
            let cut = rest.trim_end().len();
            (&trimmed[..cut], &trimmed[cut..])
        }
        None => (value, ""),
    };
    let (c, alpha) = parse_color(v)?;
    Some(format!("{}{important}", to_css(c.inverted(), alpha)))
}

/// Inside a shorthand: replace every colour token (hex, `rgb()`/`rgba()`,
/// keyword), keep widths and styles.
fn convert_tokens(value: &str) -> String {
    let word_char = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    let mut prev: Option<char> = None;
    while let Some(c) = rest.chars().next() {
        let low = rest.to_ascii_lowercase();
        let token_len = if c == '#' {
            let n = rest[1..].bytes().take_while(u8::is_ascii_hexdigit).count();
            let ends = !rest[1 + n..].starts_with(word_char);
            ((3..=8).contains(&n) && ends).then_some(1 + n)
        } else if low.starts_with("rgb(") || low.starts_with("rgba(") {
            rest.find(')').map(|i| i + 1)
        } else if c.is_ascii_alphabetic() && !prev.is_some_and(word_char) {
            let n = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
            (!rest[n..].starts_with(word_char)).then_some(n)
        } else {
            None
        };
        match token_len {
            Some(n) => {
                let token = &rest[..n];
                let is_color = token.starts_with('#')
                    || token.to_ascii_lowercase().starts_with("rgb")
                    || named(token).is_some();
                match is_color.then(|| convert_value(token)).flatten() {
                    Some(new) => out.push_str(&new),
                    None => out.push_str(token),
                }
                prev = token.chars().last();
                rest = &rest[n..];
            }
            None => {
                out.push(c);
                prev = Some(c);
                rest = &rest[c.len_utf8()..];
            }
        }
    }
    out
}

/// One `style` value with its colours inverted.
fn convert_style(style: &str) -> String {
    style
        .split(';')
        .map(|decl| {
            let Some((prop, value)) = decl.split_once(':') else {
                return decl.to_string();
            };
            let name = prop.trim().to_ascii_lowercase();
            if FLAT.contains(&name.as_str()) {
                if let Some(v) = convert_value(value) {
                    return format!("{}:{v}", prop.trim());
                }
            } else if SHORTHAND.contains(&name.as_str())
                && !value.contains("url(")
                && !value.contains("gradient")
            {
                let v = convert_tokens(value.trim());
                if v != value.trim() {
                    return format!("{}:{v}", prop.trim());
                }
            }
            decl.to_string()
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// `html` with the sender's colours inverted for a dark page.
#[must_use]
pub fn darken_colors(html: &str) -> String {
    map_tags(html, |_, attrs| {
        let mut attrs = attrs.to_string();
        if let Some((range, style)) = find_attr(&attrs, "style") {
            let new = format!(" style=\"{}\"", convert_style(style));
            attrs = splice(&attrs, range, &new);
        }
        // `bgcolor` and `color` each convert when present; an element may
        // carry both (legacy mail), so neither is skipped for the other.
        for name in ["bgcolor", "color"] {
            if let Some((range, value)) = find_attr(&attrs, name).map(|(r, v)| (r, v.to_string())) {
                if let Some(v) = convert_value(&value) {
                    attrs = splice(&attrs, range, &format!(" {name}=\"{v}\""));
                }
            }
        }
        Some(attrs)
    })
}
