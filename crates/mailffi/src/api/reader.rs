//! The reader's HTML document, from `mailcore::html::reader`: which paint a
//! mail gets, the colours it is written in, width fitting and the document
//! itself. Dart only shows the result.

use mailcore::html::reader as core;

/// How a reader paints one HTML body.
#[derive(Clone, Copy)]
pub enum ReaderPaint {
    /// The mail sets no colours: the app theme's.
    Theme,
    /// The sender's colours on the light sheet they were designed for.
    Original,
    /// The designed mail rewritten for a dark theme.
    Darkened,
}

impl From<ReaderPaint> for core::Paint {
    fn from(p: ReaderPaint) -> Self {
        match p {
            ReaderPaint::Theme => core::Paint::Theme,
            ReaderPaint::Original => core::Paint::Original,
            ReaderPaint::Darkened => core::Paint::Darkened,
        }
    }
}

/// The colours a page is written in, each `0xRRGGBB`.
#[derive(Clone, Copy)]
pub struct ReaderPalette {
    pub paper: u32,
    pub ink: u32,
    pub link: u32,
    pub quote: u32,
    pub rule: u32,
}

impl From<ReaderPalette> for core::Palette {
    fn from(p: ReaderPalette) -> Self {
        let rgb = |v: u32| core::Rgb(v & 0xFF_FFFF);
        core::Palette {
            paper: rgb(p.paper),
            ink: rgb(p.ink),
            link: rgb(p.link),
            quote: rgb(p.quote),
            rule: rgb(p.rule),
        }
    }
}

impl From<core::Palette> for ReaderPalette {
    fn from(p: core::Palette) -> Self {
        ReaderPalette {
            paper: p.paper.0,
            ink: p.ink.0,
            link: p.link.0,
            quote: p.quote.0,
            rule: p.rule.0,
        }
    }
}

/// How to build one reader document (see `reader_document`).
pub struct ReaderDocumentOptions {
    pub paint: ReaderPaint,
    /// The theme's colours: background, text, accent, muted text, border.
    pub theme: ReaderPalette,
    /// The user allowed remote images: the CSP lets them load.
    pub allow_remote: bool,
    /// Room for the header overlaying the top of the page, CSS pixels.
    pub top_space: u32,
    /// Text size multiplier.
    pub scale: f64,
    /// Loosen fixed widths (the page is narrower than `reader_fit_below`).
    pub fit: bool,
}

/// The paint for one mail: `colored` is the feed's `html_colored`,
/// `keep_original` the reader's "as sent" toggle.
#[flutter_rust_bridge::frb(sync)]
pub fn reader_paint(colored: bool, dark: bool, keep_original: bool) -> ReaderPaint {
    match core::paint_for(colored, dark, keep_original) {
        core::Paint::Theme => ReaderPaint::Theme,
        core::Paint::Original => ReaderPaint::Original,
        core::Paint::Darkened => ReaderPaint::Darkened,
    }
}

/// What `paint` writes the page in, given the theme's colours.
#[flutter_rust_bridge::frb(sync)]
pub fn reader_palette(paint: ReaderPaint, theme: ReaderPalette) -> ReaderPalette {
    core::palette(paint.into(), &theme.into()).into()
}

/// Pages narrower than this get the mail's fixed widths loosened; 0 when
/// it has none. Once per mail, not per resize.
#[flutter_rust_bridge::frb(sync)]
pub fn reader_fit_below(body: String) -> u32 {
    core::fit_below(&body)
}

/// The body as `paint` shows it, for a renderer that takes a body rather
/// than a document (the desktop widget tree).
#[flutter_rust_bridge::frb(sync)]
pub fn reader_body(body: String, paint: ReaderPaint, fit: bool) -> String {
    core::body(&body, paint.into(), fit)
}

/// The full document for a web view: CSP, base CSS, header spacer, body.
#[flutter_rust_bridge::frb(sync)]
pub fn reader_document(body: String, options: ReaderDocumentOptions) -> String {
    core::document(
        &body,
        &core::DocumentOptions {
            paint: options.paint.into(),
            theme: options.theme.into(),
            allow_remote: options.allow_remote,
            top_space: options.top_space,
            scale: options.scale as f32,
            fit: options.fit,
            extra_css: "",
        },
    )
}
