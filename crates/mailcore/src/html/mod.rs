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
//!
//! The pipeline runs left to right across the submodules:
//!
//! - [`tags`] tokenizes — which tags are allowed, and where one ends;
//! - [`entities`] decodes the entity subset and escapes text back out;
//! - [`urls`] decides which `href`/`src` values may survive at all;
//! - [`sanitize`](sanitize()) walks the input and serialises the safe result;
//! - [`text`] covers the other directions: HTML → plain, plain → HTML, and
//!   the "does this need HTML at all" heuristics the send path asks about.

mod entities;
mod sanitize;
mod tags;
mod text;
mod urls;

#[cfg(test)]
mod tests;

pub use entities::{decode_entities, escape_text};
pub use sanitize::{sanitize, sanitize_for_send};
pub use text::{html_to_text, looks_like_html, needs_html_formatting, text_to_html, wrap_document};

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
