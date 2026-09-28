//! `cid:` images: resolved to `data:` URIs from the message's own parts.
//!
//! An inline image is part of the mail (a logo, a pasted screenshot), not a
//! remote fetch. Its bytes are kept at sync time, so showing it never needs
//! the network; a reference with no stored bytes is replaced by its alt
//! text rather than left for a renderer to try and resolve.

use super::entities::{decode_entities, escape_text};

/// Largest single inline image kept, and the most one body embeds.
pub const MAX_INLINE_IMAGE_BYTES: usize = 1_500_000;
pub const MAX_INLINE_BYTES_PER_MESSAGE: usize = 6_000_000;

/// One stored inline part.
#[derive(Debug, Clone)]
pub struct InlineImage {
    pub content_id: String,
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// Image types a reader may embed: the raster set the sanitizer allows as
/// `data:` too. SVG is out, since it can carry script and references.
pub fn is_inline_image_mime(mime: &str) -> bool {
    matches!(
        mime.trim().to_ascii_lowercase().as_str(),
        "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/webp"
    )
}

/// `<id@host>`, `id@host` and `cid:id%40host` all name the same part.
pub fn normalize_content_id(raw: &str) -> String {
    let t = raw.trim();
    let t = t.strip_prefix("cid:").unwrap_or(t);
    let t = t.trim_start_matches('<').trim_end_matches('>');
    percent_decode(t).to_ascii_lowercase()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Replace every `<img src="cid:…">` in sanitized HTML with a `data:` URI
/// from `images`. Returns the new HTML and how many references had no
/// stored bytes (those become their alt text).
///
/// Input must be [`super::sanitize`] output: attribute values are quoted
/// and `>` inside them is escaped, so the next `>` ends the tag.
pub fn inline_cid_images(html: &str, images: &[InlineImage]) -> (String, usize) {
    if !html.contains("cid:") {
        return (html.to_string(), 0);
    }
    let mut out = String::with_capacity(html.len());
    let mut budget = MAX_INLINE_BYTES_PER_MESSAGE;
    let mut missing = 0;
    let mut rest = html;
    while let Some(start) = rest.find("<img") {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('>') else {
            rest = &rest[start..];
            break;
        };
        let tag = &rest[start..start + len + 1];
        rest = &rest[start + len + 1..];
        let Some(src) = attr(tag, "src") else {
            out.push_str(tag);
            continue;
        };
        let src = decode_entities(src);
        if !src.trim_start().to_ascii_lowercase().starts_with("cid:") {
            out.push_str(tag);
            continue;
        }
        let id = normalize_content_id(&src);
        let found = images.iter().find(|i| {
            normalize_content_id(&i.content_id) == id
                && is_inline_image_mime(&i.mime_type)
                && i.data.len() <= MAX_INLINE_IMAGE_BYTES
        });
        match found {
            Some(img) if img.data.len() <= budget => {
                budget -= img.data.len();
                let uri = format!(
                    "data:{};base64,{}",
                    img.mime_type.trim().to_ascii_lowercase(),
                    base64(&img.data)
                );
                let old = format!(" src=\"{}\"", attr(tag, "src").unwrap_or_default());
                out.push_str(&tag.replacen(&old, &format!(" src=\"{uri}\""), 1));
            }
            _ => {
                missing += 1;
                let alt = attr(tag, "alt").map(decode_entities).unwrap_or_default();
                let alt = alt.trim();
                if !alt.is_empty() {
                    out.push_str(&escape_text(&format!("[image: {alt}]")));
                }
            }
        }
    }
    out.push_str(rest);
    (out, missing)
}

/// The raw (still escaped) value of ` name="…"` in one sanitized tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let from = tag.find(&key)? + key.len();
    let len = tag[from..].find('"')?;
    Some(&tag[from..from + len])
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = match chunk.len() {
            3 => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]),
            2 => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8),
            _ => u32::from(chunk[0]) << 16,
        };
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::sanitize;

    fn png(id: &str, data: &[u8]) -> InlineImage {
        InlineImage {
            content_id: id.to_string(),
            mime_type: "image/png".to_string(),
            data: data.to_vec(),
        }
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn content_ids_compare_in_any_spelling() {
        assert_eq!(
            normalize_content_id("<Logo@Example.com>"),
            "logo@example.com"
        );
        assert_eq!(
            normalize_content_id("cid:logo%40example.com"),
            "logo@example.com"
        );
        assert_eq!(normalize_content_id("%4"), "%4");
    }

    #[test]
    fn cid_reference_becomes_a_data_uri() {
        let html = sanitize(
            "<p>hi<img src=\"cid:logo@example.com\" alt=\"Logo\"></p>",
            false,
        )
        .html;
        let (out, missing) = inline_cid_images(&html, &[png("<logo@example.com>", b"foo")]);
        assert_eq!(missing, 0);
        assert!(out.contains("src=\"data:image/png;base64,Zm9v\""), "{out}");
        assert!(out.contains("alt=\"Logo\""));
        assert!(!out.contains("cid:"));
    }

    #[test]
    fn missing_bytes_leave_alt_text_and_no_reference() {
        let html = sanitize(
            "<p><img src=\"cid:a@example.com\" alt=\"Chart\"><img src=\"cid:b@example.com\"></p>",
            false,
        )
        .html;
        let (out, missing) = inline_cid_images(&html, &[]);
        assert_eq!(missing, 2);
        assert!(!out.contains("cid:"));
        assert!(!out.contains("<img"));
        assert!(out.contains("[image: Chart]"));
    }

    #[test]
    fn svg_and_oversized_parts_are_not_embedded() {
        let html = sanitize("<img src=\"cid:x@example.com\">", false).html;
        let svg = InlineImage {
            mime_type: "image/svg+xml".to_string(),
            ..png("x@example.com", b"<svg/>")
        };
        assert_eq!(inline_cid_images(&html, &[svg]).1, 1);
        let big = png("x@example.com", &vec![0; MAX_INLINE_IMAGE_BYTES + 1]);
        assert_eq!(inline_cid_images(&html, &[big]).1, 1);
    }

    #[test]
    fn other_images_are_untouched() {
        let html = sanitize("<img src=\"data:image/png;base64,Zm9v\" alt=\"a\">", false).html;
        assert_eq!(inline_cid_images(&html, &[]), (html.clone(), 0));
    }
}
