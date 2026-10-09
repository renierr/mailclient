//! `cid:` images: resolved to `data:` URIs from the message's own parts.
//!
//! An inline image is part of the mail (a logo, a pasted screenshot), not a
//! remote fetch. Its bytes are kept at sync time, so showing it never needs
//! the network; a reference with no stored bytes is replaced by its alt
//! text rather than left for a renderer to try and resolve.

use super::entities::decode_entities;

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

/// `<img>` sources that point at body parts: the normalized content-IDs
/// referenced as `cid:` in raw HTML. Only `<img src>` counts — a file
/// linked from an `<a href="cid:…">` stays a real attachment, since the
/// reader embeds just images (see [`inline_cid_images`]).
///
/// Tolerant on purpose: stored bodies are raw sender HTML (single quotes,
/// no quotes, any case), not sanitized output.
pub fn img_cid_references(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut rest = &lower[..];
    // Byte slicing is safe: every byte consumed is ASCII (`<`, `img`).
    while let Some(tag) = rest.find("<img") {
        let after = &rest[tag + 4..];
        let end = after.find('>').unwrap_or(after.len());
        let tag = &after[..end];
        rest = &after[end..];
        let Some(from) = tag.find("src") else {
            continue;
        };
        let mut val = tag[from + 3..].trim_start();
        if let Some(v) = val.strip_prefix('=') {
            val = v.trim_start();
        } else {
            continue;
        }
        val = val.strip_prefix(['\'', '"']).unwrap_or(val);
        let token: String = val
            .chars()
            .take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '>' | '<'))
            .collect();
        let token = token.trim_end_matches("/>");
        if token.to_ascii_lowercase().starts_with("cid:") {
            let id = normalize_content_id(token);
            if !id.is_empty() && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// The `cid:` ids a body shows through `<img src>` ([`img_cid_references`]),
/// read once per body. A message is checked part by part, and finding the
/// references re-lowercased and re-scanned the whole body for every part,
/// up to 50 times per message open (C11).
#[derive(Debug, Default)]
pub struct BodyImages(Vec<String>);

impl BodyImages {
    pub fn of(html: Option<&str>) -> Self {
        Self(html.map(img_cid_references).unwrap_or_default())
    }

    /// Whether the part with `content_id` is one of the body's images, in
    /// any spelling (see [`normalize_content_id`]). Such a part is a body
    /// part even when the sender declared it `Content-Disposition:
    /// attachment` (newsletters do this for logos). A link
    /// (`<a href="cid:…">`) does not count.
    pub fn shows(&self, content_id: Option<&str>) -> bool {
        let Some(cid) = content_id else {
            return false;
        };
        let want = normalize_content_id(cid);
        !want.is_empty() && self.0.contains(&want)
    }
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
                    base64_encode(&img.data)
                );
                let old = format!(" src=\"{}\"", attr(tag, "src").unwrap_or_default());
                out.push_str(&tag.replacen(&old, &format!(" src=\"{uri}\""), 1));
            }
            _ => {
                missing += 1;
                let alt = attr(tag, "alt").map(decode_entities).unwrap_or_default();
                let alt = alt.trim();
                if !alt.is_empty() {
                    out.push_str(&super::sanitize::blocked_img_placeholder(alt));
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

/// Standard base64 (RFC 4648, padded).
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
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

/// Decode standard base64, tolerating whitespace and missing padding.
/// `None` on any other character.
pub(crate) fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c == b'=' {
            break;
        }
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
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
    fn base64_decode_round_trips_and_rejects_junk() {
        for input in [&b""[..], b"f", b"fo", b"foo", &[0xfb, 0xff], b"hello world"] {
            assert_eq!(base64_decode(&base64_encode(input)).unwrap(), input);
        }
        assert_eq!(base64_decode("Zm9v\r\nYmFy").unwrap(), b"foobar");
        assert!(base64_decode("Zm9v*").is_none());
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(&[0xfb, 0xff]), "+/8=");
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
        assert!(!out.contains("[image:"));
        assert!(out.contains("<details class=\"mc-blocked\">"));
        assert!(out.contains("<svg"));
        assert!(out.contains("<span>Chart</span>"));
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
    fn body_references_cover_sender_spellings() {
        let html = "<p><IMG SRC=cid:yellowLogo><img src='cid:bannerLogo'/></p>";
        let shown = BodyImages::of(Some(html));
        assert!(shown.shows(Some("yellowLogo")));
        assert!(shown.shows(Some("<bannerLogo>")));
        assert!(!shown.shows(Some("other")));
        assert!(!shown.shows(None));
        assert!(!BodyImages::of(None).shows(Some("yellowLogo")));
        // A linked file is not a body image: only <img src> counts.
        let linked = "<p><a href=\"cid:report\">report</a></p>";
        assert!(!BodyImages::of(Some(linked)).shows(Some("report")));
    }

    #[test]
    fn other_images_are_untouched() {
        let html = sanitize("<img src=\"data:image/png;base64,Zm9v\" alt=\"a\">", false).html;
        assert_eq!(inline_cid_images(&html, &[]), (html.clone(), 0));
    }
}
