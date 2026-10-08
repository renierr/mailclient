//! Outgoing inline images: the composer's `data:` images become
//! `multipart/related` parts referenced by `cid:`.
//!
//! Both editors hold an inserted image as a `data:` URI, because that is what
//! they can display. Mail clients expect a part instead — many strip `data:`
//! images, and a large one would also run into the HTML size caps — so they
//! are pulled out here, before sanitizing, and the `src` is rewritten to the
//! part's Content-ID.

use crate::error::{Result, StoreError};
use crate::html::{is_inline_image_mime, MAX_INLINE_IMAGE_BYTES};

use super::attachments::MAX_SEND_ATTACHMENT_COUNT;

/// One image to send as an inline part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlinePart {
    /// Content-ID without angle brackets (`<img src="cid:…">` names it).
    pub cid: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

impl InlinePart {
    /// File name for when the part has to travel as a plain attachment
    /// (a plain-text send has nowhere to show it inline).
    pub fn filename(&self, n: usize) -> String {
        let ext = match self.mime.as_str() {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "png",
        };
        format!("image-{n}.{ext}")
    }
}

/// Replace every `<img src="data:image/…;base64,…">` in `html` with a `cid:`
/// reference and return the parts. Identical images share one part.
/// Anything that is not a raster `data:` image is left for the sanitizer.
pub fn extract_data_images(html: &str, domain: &str) -> Result<(String, Vec<InlinePart>)> {
    if !html.contains("data:image/") {
        return Ok((html.to_string(), Vec::new()));
    }
    let mut parts: Vec<InlinePart> = Vec::new();
    let mut out = String::with_capacity(html.len().min(64 * 1024));
    let mut rest = html;
    while let Some(at) = find_ci(rest, "<img") {
        out.push_str(&rest[..at]);
        let Some(len) = tag_len(&rest[at..]) else {
            break;
        };
        let tag = &rest[at..at + len];
        rest = &rest[at + len..];
        match data_src(tag) {
            Some((start, end)) => {
                let value = &tag[start..end];
                match decode_data_image(value)? {
                    Some((mime, bytes)) => {
                        let cid = match parts.iter().find(|p| p.bytes == bytes) {
                            Some(p) => p.cid.clone(),
                            None => {
                                if parts.len() >= MAX_SEND_ATTACHMENT_COUNT {
                                    return Err(StoreError::InvalidInput(format!(
                                        "too many inline images (max {MAX_SEND_ATTACHMENT_COUNT})"
                                    )));
                                }
                                let cid = format!("{}@{}", uuid::Uuid::new_v4().simple(), domain);
                                parts.push(InlinePart {
                                    cid: cid.clone(),
                                    mime,
                                    bytes,
                                });
                                cid
                            }
                        };
                        out.push_str(&tag[..start]);
                        out.push_str("cid:");
                        out.push_str(&cid);
                        out.push_str(&tag[end..]);
                    }
                    None => out.push_str(tag),
                }
            }
            None => out.push_str(tag),
        }
    }
    out.push_str(rest);
    Ok((out, parts))
}

/// `data:` URL for an image file, for an editor to show and later send
/// inline. Refuses non-raster types and images too large to round-trip
/// through a saved draft ([`MAX_INLINE_IMAGE_BYTES`]) — those go as
/// attachments instead.
pub fn image_data_url(path_or_url: &str) -> Result<String> {
    let path = crate::paths::file_url_to_path(path_or_url.trim());
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mime = super::attachments::guess_mime(&name);
    if !is_inline_image_mime(&mime) {
        return Err(StoreError::InvalidInput(format!(
            "{name} is not an image that can be shown inline (PNG, JPEG, GIF or WebP)"
        )));
    }
    // Size first, read second. `std::fs::read` would pull the whole file into
    // memory before the check below, so dropping a multi-gigabyte video onto
    // the composer stalled the GUI thread behind an allocation to match. The
    // metadata is one `stat`, and it also tells the user the real size rather
    // than the truncated limit.
    let size = std::fs::metadata(&path)
        .map_err(|_| StoreError::InvalidInput(format!("cannot read {}", path.display())))?
        .len();
    if size > MAX_INLINE_IMAGE_BYTES as u64 {
        return Err(StoreError::InvalidInput(format!(
            "{name} is too large to insert inline ({} KB, max {} KB) — attach it instead",
            size / 1024,
            MAX_INLINE_IMAGE_BYTES / 1024
        )));
    }
    // Capped anyway: a file that grew between the `stat` and the open must not
    // be able to overrun the limit.
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .and_then(|f| {
            f.take(MAX_INLINE_IMAGE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|_| StoreError::InvalidInput(format!("cannot read {}", path.display())))?;
    if bytes.len() > MAX_INLINE_IMAGE_BYTES {
        return Err(StoreError::InvalidInput(format!(
            "{name} is too large to insert inline ({} KB, max {} KB) — attach it instead",
            bytes.len() / 1024,
            MAX_INLINE_IMAGE_BYTES / 1024
        )));
    }
    Ok(format!(
        "data:{mime};base64,{}",
        crate::html::base64_encode(&bytes)
    ))
}

/// Whether a file would be offered as an inline image (by its name).
#[must_use]
pub fn is_inline_image_file(path_or_url: &str) -> bool {
    let path = crate::paths::file_url_to_path(path_or_url.trim());
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    is_inline_image_mime(&super::attachments::guess_mime(&name))
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().find(needle)
}

/// Length of the tag starting at `s[0] == '<'`, through its `>`, skipping
/// `>` inside quoted attribute values.
fn tag_len(s: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (i, b) in s.bytes().enumerate() {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return Some(i + 1),
            None => {}
        }
    }
    None
}

/// Byte range of the `src` value inside one `<img …>` tag when it is a
/// `data:image/` URI.
fn data_src(tag: &str) -> Option<(usize, usize)> {
    let low = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find("src") {
        let at = from + i;
        from = at + 3;
        // A whole attribute name, not the tail of `data-src` or `srcset`.
        let before_ok = at > 0 && low.as_bytes()[at - 1].is_ascii_whitespace();
        let after = low[at + 3..].trim_start();
        if !before_ok || !after.starts_with('=') {
            continue;
        }
        let eq = at + 3 + (low[at + 3..].len() - after.len());
        let value_start = eq + 1 + (low[eq + 1..].len() - low[eq + 1..].trim_start().len());
        let q = low.as_bytes().get(value_start).copied()?;
        let (start, end) = if q == b'"' || q == b'\'' {
            let start = value_start + 1;
            let end = start + low[start..].find(q as char)?;
            (start, end)
        } else {
            let end = value_start
                + low[value_start..]
                    .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
                    .unwrap_or(low.len() - value_start);
            (value_start, end)
        };
        return low[start..end]
            .trim_start()
            .starts_with("data:image/")
            .then_some((start, end));
    }
    None
}

/// `(mime, bytes)` of a base64 `data:image/…` URI, `None` for a type that is
/// not sent inline (the sanitizer then drops it).
fn decode_data_image(value: &str) -> Result<Option<(String, Vec<u8>)>> {
    let v = value.trim();
    let Some(rest) = v.get(5..).filter(|_| v[..5].eq_ignore_ascii_case("data:")) else {
        return Ok(None);
    };
    let Some((meta, payload)) = rest.split_once(',') else {
        return Ok(None);
    };
    let mut fields = meta.split(';');
    let mime = fields.next().unwrap_or("").trim().to_ascii_lowercase();
    if !fields.any(|f| f.trim().eq_ignore_ascii_case("base64")) || !is_inline_image_mime(&mime) {
        return Ok(None);
    }
    let bytes = crate::html::base64_decode(payload)
        .ok_or_else(|| StoreError::InvalidInput("an inline image could not be read".to_string()))?;
    if bytes.len() as u64 > super::attachments::MAX_SEND_ATTACHMENT_BYTES {
        return Err(StoreError::InvalidInput(
            "an inline image is larger than the attachment limit".to_string(),
        ));
    }
    Ok(Some((mime, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_images_become_cid_parts_and_duplicates_share_one() {
        let html = r#"<p>a<img alt="x" src="data:image/png;base64,Zm9v">b<IMG SRC='data:image/png;base64,Zm9v'></p>"#;
        let (out, parts) = extract_data_images(html, "example.com").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].bytes, b"foo");
        assert_eq!(parts[0].mime, "image/png");
        assert!(parts[0].cid.ends_with("@example.com"));
        assert_eq!(
            out.matches(&format!("cid:{}", parts[0].cid)).count(),
            2,
            "{out}"
        );
        assert!(!out.contains("data:"));
    }

    #[test]
    fn other_sources_are_left_alone() {
        for html in [
            r#"<img src="https://example.com/a.png">"#,
            r#"<img data-src="data:image/png;base64,Zm9v" src="cid:x">"#,
            r#"<img src="data:image/svg+xml;base64,Zm9v">"#,
            r#"<img src="data:image/png,raw">"#,
            "<p>no images</p>",
        ] {
            let (out, parts) = extract_data_images(html, "example.com").unwrap();
            assert!(parts.is_empty(), "{html}");
            assert_eq!(out, html);
        }
    }

    #[test]
    fn broken_base64_is_an_error_not_a_silent_drop() {
        assert!(extract_data_images(r#"<img src="data:image/png;base64,***">"#, "e.com").is_err());
    }

    #[test]
    fn image_files_become_data_urls_and_others_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("logo.png");
        std::fs::write(&png, b"foo").unwrap();
        let txt = dir.path().join("notes.txt");
        std::fs::write(&txt, b"foo").unwrap();
        assert_eq!(
            image_data_url(&png.to_string_lossy()).unwrap(),
            "data:image/png;base64,Zm9v"
        );
        assert!(is_inline_image_file(&png.to_string_lossy()));
        assert!(!is_inline_image_file(&txt.to_string_lossy()));
        assert!(image_data_url(&txt.to_string_lossy()).is_err());
        let big = dir.path().join("big.jpg");
        std::fs::write(&big, vec![0u8; MAX_INLINE_IMAGE_BYTES + 1]).unwrap();
        assert!(image_data_url(&big.to_string_lossy()).is_err());
    }

    #[test]
    fn an_oversized_file_is_refused_before_it_is_read() {
        // The old `std::fs::read` pulled the whole file into memory before
        // the size check, so a multi-gigabyte file stalled the GUI thread
        // behind an allocation to match. A sparse file keeps the test cheap
        // while still being larger than the limit.
        let dir = tempfile::tempdir().unwrap();
        let huge = dir.path().join("huge.png");
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&huge)
                .unwrap();
            f.write_all(b"x").unwrap();
            f.seek(SeekFrom::Start(4 * 1024 * 1024 * 1024)).unwrap();
            f.write_all(b"y").unwrap();
            f.flush().unwrap();
        }
        let err = image_data_url(&huge.to_string_lossy())
            .expect_err("must be refused")
            .to_string();
        assert!(err.contains("too large"), "{err}");
        // The real size is reported, not the truncated limit, so the user
        // learns *why* their file was refused.
        assert!(err.contains("4194304 KB"), "{err}");
    }

    #[test]
    fn a_file_that_grows_past_the_limit_is_still_refused() {
        // Belt and braces: the capped read must not let a file that grew
        // between the `stat` and the open overrun the limit.
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("grows.png");
        std::fs::write(&png, vec![0u8; 64]).unwrap();
        let url = image_data_url(&png.to_string_lossy()).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        // Well under the limit, so nothing else applies.
        assert!(url.len() < MAX_INLINE_IMAGE_BYTES);
    }
}
