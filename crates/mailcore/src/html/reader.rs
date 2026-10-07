//! The reader's HTML document, built once for both frontends.
//!
//! Input is always [`super::sanitize`] output. What is decided here: which
//! paint a mail gets ([`paint_for`]), the colours it is written in
//! ([`palette`]), the dark rewrite of a designed mail ([`dark`]), loosening
//! fixed-width layouts for a narrow page ([`fit`]), and the trusted document
//! around the body — CSP, base CSS, the header spacer ([`document`]). How
//! the page reaches the screen (WebEngine, WebView, a widget tree) and how
//! it scrolls stays with each toolkit.
//!
//! A darkened mail is rewritten up front instead of being run through a CSS
//! `filter`: a filter re-renders through itself on every scrolled frame,
//! which is the stutter dark mails show; rewritten colour values leave the
//! compositor nothing to redo, and images keep their real colours without a
//! second inversion.

mod attrs;
mod dark;
mod fit;
mod named_colors;

#[cfg(test)]
mod tests;

use serde::Serialize;

use super::entities::escape_attr;

pub use dark::darken_colors;
pub use fit::{fit_widths, layout_width};

/// How a reader paints one HTML body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Paint {
    /// The mail sets no colours: the app theme's, like a plain-text mail.
    Theme,
    /// The sender's colours on the light sheet they were designed for.
    Original,
    /// The designed mail rewritten for a dark theme ([`darken_colors`]).
    Darkened,
}

impl Paint {
    /// `theme`, `original` or `darkened`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Paint::Theme => "theme",
            Paint::Original => "original",
            Paint::Darkened => "darkened",
        }
    }

    /// The inverse of [`Paint::as_str`]; anything else is [`Paint::Theme`].
    #[must_use]
    pub fn parse(s: &str) -> Paint {
        match s {
            "original" => Paint::Original,
            "darkened" => Paint::Darkened,
            _ => Paint::Theme,
        }
    }
}

/// The paint for one mail. `colored` is the feed's `html_colored`;
/// `keep_original` is the reader's per-message "as sent" toggle.
#[must_use]
pub fn paint_for(colored: bool, dark: bool, keep_original: bool) -> Paint {
    if !colored {
        Paint::Theme
    } else if dark && !keep_original {
        Paint::Darkened
    } else {
        Paint::Original
    }
}

/// An opaque colour, `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u32);

impl Rgb {
    /// `#rgb` or `#rrggbb`; `None` for anything else, so a frontend can
    /// never smuggle more than a colour into the document's CSS.
    #[must_use]
    pub fn parse(css: &str) -> Option<Rgb> {
        let hex = css.trim().strip_prefix('#')?;
        if !hex.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        match hex.len() {
            6 => u32::from_str_radix(hex, 16).ok().map(Rgb),
            3 => {
                let wide: String = hex.chars().flat_map(|c| [c, c]).collect();
                u32::from_str_radix(&wide, 16).ok().map(Rgb)
            }
            _ => None,
        }
    }

    /// `#rrggbb`.
    #[must_use]
    pub fn css(self) -> String {
        format!("#{:06x}", self.0 & 0xFF_FFFF)
    }

    fn channels(self) -> [f64; 3] {
        [16, 8, 0].map(|s| f64::from((self.0 >> s) & 0xFF) / 255.0)
    }

    /// The colour `invert(1) hue-rotate(180deg)` makes of this one:
    /// lightness flips, hues stay (red text stays red). Applied twice it
    /// gives the colour back.
    #[must_use]
    pub fn inverted(self) -> Rgb {
        let [r, g, b] = invert(self.channels());
        Rgb((to_byte(r) << 16) | (to_byte(g) << 8) | to_byte(b))
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.css())
    }
}

/// `invert(1) hue-rotate(180deg)` on channels in `0.0..=1.0`.
fn invert([r, g, b]: [f64; 3]) -> [f64; 3] {
    [
        1.0 - (-0.574 * r + 1.430 * g + 0.144 * b),
        1.0 - (0.426 * r + 0.430 * g + 0.144 * b),
        1.0 - (0.426 * r + 1.430 * g - 0.856 * b),
    ]
}

fn to_byte(v: f64) -> u32 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u32
}

/// The colours a document is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Palette {
    pub paper: Rgb,
    pub ink: Rgb,
    pub link: Rgb,
    pub quote: Rgb,
    pub rule: Rgb,
}

impl Palette {
    /// From CSS colours (`#rrggbb`), as a frontend reads them off its theme.
    /// One that does not parse keeps the light sheet's.
    #[must_use]
    pub fn from_css(paper: &str, ink: &str, link: &str, quote: &str, rule: &str) -> Palette {
        let or = |css: &str, fallback: Rgb| Rgb::parse(css).unwrap_or(fallback);
        Palette {
            paper: or(paper, LIGHT.paper),
            ink: or(ink, LIGHT.ink),
            link: or(link, LIGHT.link),
            quote: or(quote, LIGHT.quote),
            rule: or(rule, LIGHT.rule),
        }
    }
}

/// The light sheet designed mail expects.
pub const LIGHT: Palette = Palette {
    paper: Rgb(0xFFFFFF),
    ink: Rgb(0x202124),
    link: Rgb(0x1A5FD0),
    quote: Rgb(0x5F6368),
    rule: Rgb(0xD0D4DA),
};

/// What `paint` writes the page in, given the frontend's theme colours
/// (background, text, accent, muted text, border). Darkened mail sits on
/// the theme's background with the light sheet's colours inverted, the
/// same colours the sender's own get from [`darken_colors`].
#[must_use]
pub fn palette(paint: Paint, theme: &Palette) -> Palette {
    match paint {
        Paint::Theme => *theme,
        Paint::Original => LIGHT,
        Paint::Darkened => Palette {
            paper: theme.paper,
            ink: LIGHT.ink.inverted(),
            link: LIGHT.link.inverted(),
            quote: LIGHT.quote.inverted(),
            rule: LIGHT.rule.inverted(),
        },
    }
}

/// The page's own side margin, in CSS pixels; a layout must fit inside
/// the page minus both.
pub const PAGE_MARGIN_PX: u32 = 16;
/// Body text size at scale 1, in CSS pixels.
pub const BASE_FONT_PX: f32 = 14.0;

/// Pages narrower than this get the mail's fixed widths loosened
/// ([`fit_widths`]); `0` when the mail has none. Cheap to compare on every
/// resize — compute it once per mail.
#[must_use]
pub fn fit_below(body: &str) -> u32 {
    match layout_width(body) {
        0 => 0,
        w => w + 2 * PAGE_MARGIN_PX,
    }
}

/// The body as `paint` shows it: fixed widths loosened when `fit`, the
/// sender's colours rewritten when darkened. For renderers that take a body
/// rather than a document (a widget tree); [`document`] does this itself.
#[must_use]
pub fn body(body: &str, paint: Paint, fit: bool) -> String {
    let fitted = if fit {
        fit_widths(body)
    } else {
        body.to_string()
    };
    if paint == Paint::Darkened {
        darken_colors(&fitted)
    } else {
        fitted
    }
}

/// How to build one reader document.
#[derive(Debug, Clone)]
pub struct DocumentOptions<'a> {
    pub paint: Paint,
    /// The frontend's theme colours (see [`palette`]).
    pub theme: Palette,
    /// The user allowed remote images: the CSP lets them load.
    pub allow_remote: bool,
    /// Height of the spacer that leaves room for the header overlaying the
    /// top of the page, in CSS pixels.
    pub top_space: u32,
    /// Text size multiplier on [`BASE_FONT_PX`].
    pub scale: f32,
    /// Loosen fixed widths (see [`fit_below`]).
    pub fit: bool,
    /// Toolkit CSS appended last (scrollbar styling). Trusted, but kept
    /// from closing the style element.
    pub extra_css: &'a str,
}

/// The full document handed to a web engine: CSP first, then the sheet
/// styling, then the sanitized body. Nothing from the mail can reach
/// `<head>` (the sanitizer drops `head`, `meta` and `style`), and a second
/// CSP could only narrow this one.
#[must_use]
pub fn document(sanitized: &str, opts: &DocumentOptions<'_>) -> String {
    let p = palette(opts.paint, &opts.theme);
    // `cid:` images arrive already embedded as `data:` by the core.
    let img = if opts.allow_remote {
        "data: https: http:"
    } else {
        "data:"
    };
    let csp = format!(
        "default-src 'none'; img-src {img}; style-src 'unsafe-inline'; font-src 'none'; \
         media-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'"
    );
    let scale = if opts.scale.is_finite() && opts.scale > 0.0 {
        opts.scale
    } else {
        1.0
    };
    let font = (BASE_FONT_PX * scale).round() as u32;
    let m = PAGE_MARGIN_PX;
    let (paper, ink, link, quote, rule) = (
        p.paper.css(),
        p.ink.css(),
        p.link.css(),
        p.quote.css(),
        p.rule.css(),
    );
    // Loosened widths are caps; padding must fit inside them.
    let fit_css = if opts.fit {
        "div,table{box-sizing:border-box}"
    } else {
        ""
    };
    let extra = opts.extra_css.replace('<', "");
    let content = body(sanitized, opts.paint, opts.fit);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\">\
         <meta http-equiv=\"x-dns-prefetch-control\" content=\"off\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <style>html,body{{background:{paper}}}\
         body{{margin:0 {m}px {m}px;background:{paper};color:{ink};font-family:sans-serif;\
         font-size:{font}px;line-height:1.5;overflow-wrap:break-word}}\
          #mc-top{{margin-bottom:{m}px}}a{{color:{link}}}\
          img{{max-width:100%!important;height:auto!important}}\
          .mc-blocked{{display:inline-block;max-width:min(160px,100%);vertical-align:middle;font-size:12px;line-height:1.4;box-sizing:border-box}}\
          .mc-blocked>summary{{list-style:none;cursor:pointer;display:inline-block;line-height:0}}\
          .mc-blocked>summary::-webkit-details-marker{{display:none}}\
          .mc-blocked>summary::marker{{content:\"\"}}\
          .mc-blocked svg{{display:block;width:64px;height:48px;max-width:100%;opacity:.7}}\
          .mc-blocked>span{{display:block;max-width:220px;white-space:normal;overflow-wrap:anywhere;font-size:12px;line-height:1.4;opacity:.8;margin-top:4px}}\
          table{{max-width:100%!important}}td,th{{overflow-wrap:anywhere}}{fit_css}\
         pre{{white-space:pre-wrap}}\
         blockquote{{margin:8px 0;padding-left:12px;border-left:3px solid {rule};color:{quote}}}\
         {extra}</style></head><body><div id=\"mc-top\" style=\"height:{top}px\"></div>\
         {content}</body></html>",
        csp = escape_attr(&csp),
        top = opts.top_space,
    )
}
