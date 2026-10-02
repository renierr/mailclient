//! Fixed-width newsletter layouts on a page narrower than their design.
//!
//! Designed mail is laid out at 600–700px and says so with pixel `width`
//! attributes and inline styles. The document CSS caps tables and images at
//! the page width, but in table layout a cell's pixel width is a *minimum*,
//! so a row of fixed cells (or a `min-width`) still pushes the page sideways.
//! When the layout is wider than the page, [`fit_widths`] turns those fixed
//! widths into caps so it can shrink; where the mail fits, it stays exactly
//! as designed. Only widths change — nothing here can add content, a URL,
//! or anything the sanitizer would not have let through.

use super::attrs::{find_attr, map_tags, splice, tags};

/// Cells take their width from the table once their own is gone.
fn is_cell(tag: &str) -> bool {
    matches!(tag, "td" | "th" | "col")
}

/// A `width="600"` attribute: bare pixels only (percentages do not count).
fn pixel_attr(attrs: &str) -> Option<(std::ops::Range<usize>, u32)> {
    let (range, value) = find_attr(attrs, "width")?;
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((range, value.parse().ok()?))
}

/// A `width:600px` / `min-width:600px` declaration: which, and the pixels.
fn pixel_decl(decl: &str) -> Option<(bool, f64)> {
    let (prop, value) = decl.split_once(':')?;
    let min = match prop.trim().to_ascii_lowercase().as_str() {
        "width" => false,
        "min-width" => true,
        _ => return None,
    };
    let number = value.trim().strip_suffix("px")?.trim_end();
    let digits = number.split_once('.').map_or(number, |(int, frac)| {
        if frac.is_empty() || !frac.bytes().all(|c| c.is_ascii_digit()) {
            ""
        } else {
            int
        }
    });
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((min, number.parse().ok()?))
}

/// Widest fixed pixel width the layout asks for: tables, cells, blocks.
/// Images are left out — the document CSS already shrinks them. `0` for
/// mail with no fixed widths.
#[must_use]
pub fn layout_width(html: &str) -> u32 {
    let mut widest = 0u32;
    for (tag, attrs) in tags(html) {
        if tag == "img" {
            continue;
        }
        if let Some((_, w)) = pixel_attr(attrs) {
            widest = widest.max(w);
        }
        if let Some((_, style)) = find_attr(attrs, "style") {
            for (_, px) in style.split(';').filter_map(pixel_decl) {
                widest = widest.max(px.ceil() as u32);
            }
        }
    }
    widest
}

/// `html` with fixed widths loosened so the layout fits a narrow page:
/// cells lose their pixel widths (the table shares out what is there),
/// other elements keep theirs as a `max-width` and otherwise fill their
/// container, and pixel `min-width`s go.
#[must_use]
pub fn fit_widths(html: &str) -> String {
    map_tags(html, |tag, attrs| {
        if tag == "img" {
            return None;
        }
        let cell = is_cell(tag);
        let mut attrs = attrs.to_string();
        let mut cap = String::new();
        if let Some((range, w)) = pixel_attr(&attrs) {
            if !cell {
                cap = format!("width:100%;max-width:{w}px;");
            }
            attrs = splice(&attrs, range, "");
        }
        if let Some((range, style)) = find_attr(&attrs, "style") {
            let kept: Vec<String> = style
                .split(';')
                .map(|decl| match pixel_decl(decl) {
                    Some((min, _)) if cell || min => String::new(),
                    Some((_, px)) => format!("width:100%;max-width:{px}px"),
                    None => decl.to_string(),
                })
                .collect();
            // The element's own style still wins over the attribute's cap.
            let style = format!(" style=\"{cap}{}\"", kept.join(";"));
            attrs = splice(&attrs, range, &style);
        } else if !cap.is_empty() {
            attrs.push_str(&format!(" style=\"{cap}\""));
        }
        Some(attrs)
    })
}
