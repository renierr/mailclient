//! Bytes know what they are: magic-byte sniffing and MIME ↔ extension maps.
//!
//! Attachment metadata comes from the mail, so it is attacker-chosen and
//! gets checked rather than trusted. Two spots lie in practice:
//!
//! - a MIME header of `application/octet-stream` (or nothing) on a real
//!   PNG/PDF, and worse, a confident but wrong type;
//! - a filename like `inline` with no extension, so the OS opener guesses
//!   the application by extension and picks wrong.
//!
//! [`sniff_mime`] reads the first bytes (PNG/JPEG/GIF/WebP/PDF/ZIP only —
//! std only, no new dependency), [`corrected_mime`] lets definitive magic
//! overrule a missing, generic or mismatched header, and
//! [`extension_for_mime`] + the MIME-aware filename in `paths` make
//! sure the file written for it carries a matching extension.

/// Magic bytes → MIME, for the common types only. Returns `None` when the
/// prefix is unknown — notably for text and SVG, which have no magic.
pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        return Some("image/png");
    }
    if bytes.len() >= 3 && &bytes[..3] == b"\xff\xd8\xff" {
        return Some("image/jpeg");
    }
    if bytes.len() >= 6 && (&bytes[..6] == b"GIF87a" || &bytes[..6] == b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 5 && &bytes[..5] == b"%PDF-" {
        return Some("application/pdf");
    }
    if bytes.len() >= 4
        && (&bytes[..4] == b"PK\x03\x04"
            || &bytes[..4] == b"PK\x05\x06"
            || &bytes[..4] == b"PK\x07\x08")
    {
        return Some("application/zip");
    }
    None
}

/// Normalize a declared MIME for comparison (`IMAGE/JPG` ≡ `image/jpeg`).
fn normalize_declared(mime: &str) -> String {
    let m = mime.trim().to_ascii_lowercase();
    match m.as_str() {
        "image/jpg" => "image/jpeg".to_string(),
        _ => m,
    }
}

/// Whether a declared header carries no information.
fn is_generic(mime: &str) -> bool {
    let m = mime.trim().to_ascii_lowercase();
    m.is_empty() || m == "application/octet-stream" || m == "application/unknown"
}

/// The MIME to store for bytes with a declared header: the sniffed type
/// when magic is definitive and the header is missing, generic or a
/// different type — except that a ZIP sniff never downgrades a more
/// specific declared type (docx/xlsx/odt/epub are all ZIPs).
pub fn corrected_mime(declared: Option<&str>, bytes: &[u8]) -> Option<String> {
    let sniffed = sniff_mime(bytes)?;
    let declared = declared.unwrap_or("").trim();
    if sniffed == "application/zip" {
        if is_generic(declared) {
            return Some(sniffed.to_string());
        }
        return None;
    }
    if declared.is_empty() || is_generic(declared) || normalize_declared(declared) != sniffed {
        return Some(sniffed.to_string());
    }
    None
}

/// File extension (no dot, lowercase) → MIME, the sender's `guess_mime`
/// table as a lookup. `None` for unknown extensions.
pub fn mime_for_extension(ext: &str) -> Option<&'static str> {
    Some(match ext.trim().to_ascii_lowercase().as_str() {
        "txt" | "log" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "csv" => "text/csv",
        "ics" => "text/calendar",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" | "pptx" => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        }
        _ => return None,
    })
}
/// MIME → file extension (no dot), mirroring the sender's `guess_mime`
/// table the other way round.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime.trim().to_ascii_lowercase().as_str() {
        "text/plain" => "txt",
        "text/html" => "html",
        "text/csv" => "csv",
        "text/calendar" => "ics",
        "application/pdf" => "pdf",
        "application/json" => "json",
        "application/zip" => "zip",
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "audio/mpeg" => "mp3",
        "video/mp4" => "mp4",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.ms-powerpoint"
        | "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing_covers_the_common_magic() {
        assert_eq!(sniff_mime(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(sniff_mime(b"\xff\xd8\xff\xe0"), Some("image/jpeg"));
        assert_eq!(sniff_mime(b"GIF89a..."), Some("image/gif"));
        assert_eq!(
            sniff_mime(b"RIFF\x00\x00\x00\x00WEBP..."),
            Some("image/webp")
        );
        assert_eq!(sniff_mime(b"%PDF-1.7..."), Some("application/pdf"));
        assert_eq!(sniff_mime(b"PK\x03\x04..."), Some("application/zip"));
        assert_eq!(sniff_mime(b"hello world"), None);
        assert_eq!(sniff_mime(b""), None);
        assert_eq!(sniff_mime(b"\xff\xd8"), None);
    }

    #[test]
    fn correction_fills_generic_and_fixes_mismatches() {
        let png = b"\x89PNG\r\n\x1a\nxxxx";
        assert_eq!(corrected_mime(None, png).as_deref(), Some("image/png"));
        assert_eq!(
            corrected_mime(Some("application/octet-stream"), png).as_deref(),
            Some("image/png")
        );
        assert_eq!(
            corrected_mime(Some("image/jpeg"), png).as_deref(),
            Some("image/png")
        );
        // Already right: no change reported.
        assert_eq!(corrected_mime(Some("image/png"), png), None);
        assert_eq!(corrected_mime(Some("IMAGE/JPG"), b"\xff\xd8\xff!"), None);
        // ZIP never downgrades a specific office type.
        assert_eq!(
            corrected_mime(
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
                b"PK\x03\x04..."
            ),
            None
        );
        assert_eq!(
            corrected_mime(Some("application/octet-stream"), b"PK\x03\x04...").as_deref(),
            Some("application/zip")
        );
        // Unknown magic: header stands.
        assert_eq!(corrected_mime(Some("text/plain"), b"hi"), None);
    }

    #[test]
    fn extensions_cover_unknown_exts() {
        assert_eq!(mime_for_extension("PDF"), Some("application/pdf"));
        assert_eq!(mime_for_extension("jpg"), Some("image/jpeg"));
        assert_eq!(mime_for_extension("dat"), None);
        assert_eq!(mime_for_extension(""), None);
    }

    #[test]
    fn extensions_round_trip_the_sender_guesses() {
        assert_eq!(extension_for_mime("image/png"), Some("png"));
        assert_eq!(extension_for_mime("image/jpeg"), Some("jpg"));
        assert_eq!(extension_for_mime("IMAGE/JPG"), Some("jpg"));
        assert_eq!(extension_for_mime("application/pdf"), Some("pdf"));
        assert_eq!(extension_for_mime("application/octet-stream"), None);
    }
}
