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
//! [`sniff_mime`] reads the first bytes (images, documents, archives,
//! audio/video containers and text calendar/contact/mail shapes — std only,
//! no new dependency), [`corrected_mime`] lets definitive magic overrule a
//! missing, generic or mismatched header, and [`extension_for_mime`] +
//! the MIME-aware filename in `paths` make sure the file written for it
//! carries a matching extension.

/// Calendar MIME spellings seen in the wild, all meaning the same event
/// file. Canonical is `text/calendar` (RFC 5545): `application/ics` is the
/// old Outlook spelling, the `x-` forms predate standardisation.
const CALENDAR_MIMES: &[&str] = &[
    "text/calendar",
    "application/ics",
    "text/x-vcalendar",
    "application/x-vcalendar",
    "text/x-vcal",
];

/// Contact card spellings; canonical is `text/vcard`.
const VCARD_MIMES: &[&str] = &["text/vcard", "text/x-vcard", "text/directory"];

/// Whether `mime` names a calendar file in any known spelling.
pub fn is_calendar_mime(mime: &str) -> bool {
    CALENDAR_MIMES.contains(&mime.trim().to_ascii_lowercase().as_str())
}

/// The canonical MIME for `mime` when it is a known alias
/// (`application/ics` → `text/calendar`, `image/jpg` → `image/jpeg`,
/// `text/x-vcard` → `text/vcard`); otherwise the trimmed lowercased input.
pub fn canonical_mime(mime: &str) -> String {
    let m = mime.trim().to_ascii_lowercase();
    if CALENDAR_MIMES.contains(&m.as_str()) {
        return "text/calendar".to_string();
    }
    if VCARD_MIMES.contains(&m.as_str()) {
        return "text/vcard".to_string();
    }
    match m.as_str() {
        "image/jpg" => "image/jpeg".to_string(),
        "image/x-png" => "image/png".to_string(),
        "audio/x-mp3" => "audio/mpeg".to_string(),
        "video/x-msvideo" => "video/x-msvideo".to_string(),
        _ => m,
    }
}

/// Leading ASCII whitespace skipped before text-shape sniffing, so a
/// BOM or blank line does not hide a `BEGIN:VCALENDAR`.
fn text_head(bytes: &[u8]) -> &[u8] {
    let mut i = 0;
    // UTF-8 BOM.
    if bytes.len() >= 3 && &bytes[..3] == b"\xef\xbb\xbf" {
        i = 3;
    }
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    &bytes[i..]
}

/// Upper-cased ASCII prefix of `text_head`, for case-insensitive matches.
fn head_upper(bytes: &[u8], n: usize) -> Vec<u8> {
    text_head(bytes)
        .iter()
        .take(n)
        .map(u8::to_ascii_uppercase)
        .collect()
}

/// Magic bytes → MIME, for the common types only. Returns `None` when the
/// prefix is unknown — notably for plain text and CSV, which have no magic.
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
    // BMP, ICO, TIFF (both byte orders).
    if bytes.len() >= 2 && &bytes[..2] == b"BM" {
        return Some("image/bmp");
    }
    if bytes.len() >= 4 && &bytes[..4] == b"\x00\x00\x01\x00" {
        return Some("image/x-icon");
    }
    if bytes.len() >= 4 && (&bytes[..4] == b"II*\x00" || &bytes[..4] == b"MM\x00*") {
        return Some("image/tiff");
    }
    // HEIC/HEIF/AVIF ride in an ISO BMFF `ftyp` box.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"heic" || brand == b"heix" || brand == b"hevc" || brand == b"heim" {
            return Some("image/heic");
        }
        if brand == b"avif" {
            return Some("image/avif");
        }
        if brand == b"mif1" || brand == b"msf1" {
            return Some("image/heif");
        }
        // `isom`/`mp41`/… below is a video container, handled as MP4.
    }
    if bytes.len() >= 5 && &bytes[..5] == b"%PDF-" {
        return Some("application/pdf");
    }
    if bytes.len() >= 4
        && (&bytes[..4] == b"PK\x03\x04"
            || &bytes[..4] == b"PK\x05\x06"
            || &bytes[..4] == b"PK\x07\x08")
    {
        return Some(zip_subtype(bytes));
    }
    // 7z, RAR, gzip, bzip2, xz.
    if bytes.len() >= 6 && &bytes[..6] == b"7z\xbc\xaf'\x1c" {
        return Some("application/x-7z-compressed");
    }
    if bytes.len() >= 7 && &bytes[..7] == b"Rar!\x1a\x07\x00" {
        return Some("application/vnd.rar");
    }
    if bytes.len() >= 2 && &bytes[..2] == b"\x1f\x8b" {
        return Some("application/gzip");
    }
    if bytes.len() >= 3 && &bytes[..3] == b"BZh" {
        return Some("application/x-bzip2");
    }
    if bytes.len() >= 6 && &bytes[..6] == b"\xfd7zXZ\x00" {
        return Some("application/x-xz");
    }
    // Audio: MP3 (ID3 or frame sync), OGG, FLAC, WAV/AVI (`RIFF…WAVE/AVI`).
    if bytes.len() >= 3 && (&bytes[..3] == b"ID3" || (bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0))
    {
        return Some("audio/mpeg");
    }
    if bytes.len() >= 4 && &bytes[..4] == b"OggS" {
        return Some("audio/ogg");
    }
    if bytes.len() >= 4 && &bytes[..4] == b"fLaC" {
        return Some("audio/flac");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" {
        if &bytes[8..12] == b"WAVE" {
            return Some("audio/wav");
        }
        if &bytes[8..12] == b"AVI " {
            return Some("video/x-msvideo");
        }
        // Otherwise it was already handled as WebP above, or unknown.
    }
    // Video containers: MP4 (`ftyp` + video brand), Matroska/WebM (EBML).
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Some("video/mp4");
    }
    if bytes.len() >= 4 && &bytes[..4] == b"\x1a\x45\xdf\xa3" {
        return Some("video/x-matroska");
    }
    // Text shapes with a fixed lead: calendar, contact, mail, markup.
    let head = head_upper(bytes, 15);
    if head.starts_with(b"BEGIN:VCALENDAR") || head.starts_with(b"BEGIN:VEVENT") {
        return Some("text/calendar");
    }
    if head.starts_with(b"BEGIN:VCARD") {
        return Some("text/vcard");
    }
    if head.starts_with(b"<SVG") || head.starts_with(b"<?XML") && contains_tag(bytes, b"<svg") {
        return Some("image/svg+xml");
    }
    if head.starts_with(b"<!DOCTYPE HTML") || head.starts_with(b"<HTML") {
        return Some("text/html");
    }
    if head.starts_with(b"<?XML") {
        return Some("application/xml");
    }
    if looks_like_eml(bytes) {
        return Some("message/rfc822");
    }
    None
}

/// Whether the lowercased haystack holds ASCII `needle`.
fn contains_tag(bytes: &[u8], needle: &[u8]) -> bool {
    let head = text_head(bytes);
    let limit = head.len().min(512);
    head[..limit]
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

/// An `.eml`'s first headers: `From:`/`To:`/`Subject:`/`Date:`/`Received:` /
/// `MIME-Version:` / `Message-ID:` / `Return-Path:` before the first blank
/// line. Bounded to the first 2 KiB so a long body cannot fake it.
fn looks_like_eml(bytes: &[u8]) -> bool {
    let end = bytes
        .windows(2)
        .position(|w| w == b"\r\n\r\n" || w == b"\n\n")
        .map(|i| i.min(2048))
        .unwrap_or(bytes.len().min(2048));
    let head = &bytes[..end];
    let upper: Vec<u8> = head.iter().map(|b| b.to_ascii_uppercase()).collect();
    const HEADERS: &[&[u8]] = &[
        b"\nFROM:",
        b"\nTO:",
        b"\nSUBJECT:",
        b"\nDATE:",
        b"\nRECEIVED:",
        b"\nMESSAGE-ID:",
        b"\nMIME-VERSION:",
        b"\nRETURN-PATH:",
        b"\nDELIVERED-TO:",
    ];
    // Also match a header on the very first line (no leading newline).
    let mut padded = Vec::with_capacity(upper.len() + 1);
    padded.push(b'\n');
    padded.extend_from_slice(&upper);
    HEADERS
        .iter()
        .any(|h| padded.windows(h.len()).any(|w| w == *h))
}

/// A ZIP's specific office/document subtype from the filenames stored in
/// its local headers (`word/document.xml` → docx, …). Falls back to plain
/// `application/zip` when nothing matches — notably for real archives.
fn zip_subtype(bytes: &[u8]) -> &'static str {
    // Only the first 64 KiB are scanned: enough for the first central
    // entries of a small office file, bounded for a large archive.
    let end = bytes.len().min(64 * 1024);
    let hay = &bytes[..end];
    let has = |needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
    if has(b"word/document.xml") {
        return "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    }
    if has(b"xl/workbook.xml") {
        return "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
    }
    if has(b"ppt/presentation.xml") {
        return "application/vnd.openxmlformats-officedocument.presentationml.presentation";
    }
    if has(b"mimetypeapplication/vnd.oasis.opendocument.text") {
        return "application/vnd.oasis.opendocument.text";
    }
    if has(b"mimetypeapplication/vnd.oasis.opendocument.spreadsheet") {
        return "application/vnd.oasis.opendocument.spreadsheet";
    }
    if has(b"mimetypeapplication/vnd.oasis.opendocument.presentation") {
        return "application/vnd.oasis.opendocument.presentation";
    }
    if has(b"mimetypeapplication/epub+zip") {
        return "application/epub+zip";
    }
    "application/zip"
}

/// Normalize a declared MIME for comparison (`IMAGE/JPG` ≡ `image/jpeg`,
/// `application/ics` ≡ `text/calendar`).
fn normalize_declared(mime: &str) -> String {
    canonical_mime(mime)
}

/// Whether a declared header carries no information.
fn is_generic(mime: &str) -> bool {
    let m = mime.trim().to_ascii_lowercase();
    m.is_empty()
        || m == "application/octet-stream"
        || m == "application/unknown"
        || m == "application/binary"
}

/// The MIME to store for bytes with a declared header: the sniffed type
/// when magic is definitive and the header is missing, generic or a
/// different type — except that a plain-ZIP sniff never downgrades a more
/// specific declared office type (a declared docx stays a docx; a generic
/// header on docx bytes upgrades to the sniffed docx subtype instead).
pub fn corrected_mime(declared: Option<&str>, bytes: &[u8]) -> Option<String> {
    let sniffed = sniff_mime(bytes)?;
    let declared = declared.unwrap_or("").trim();
    if sniffed == "application/zip" {
        if is_generic(declared) {
            return Some(sniffed.to_string());
        }
        return None;
    }
    // A ZIP-subtype sniff (docx/xlsx/…) is more specific than a plain ZIP
    // or generic header, but never overrules a different specific type.
    if sniffed.starts_with("application/vnd.")
        || sniffed == "application/epub+zip"
        || sniffed == "application/vnd.rar"
    {
        if declared.is_empty()
            || is_generic(declared)
            || normalize_declared(declared) == "application/zip"
        {
            return Some(sniffed.to_string());
        }
        if normalize_declared(declared) != sniffed {
            return None;
        }
        return None;
    }
    if declared.is_empty() || is_generic(declared) || normalize_declared(declared) != sniffed {
        return Some(sniffed.to_string());
    }
    None
}

/// Repair a declared header without bytes: canonicalize known aliases
/// (`application/ics` → `text/calendar`) and fill a missing/generic header
/// from the filename extension. Returns the MIME to store, or `None` when
/// the declared header already stands.
pub fn repaired_mime(declared: Option<&str>, filename: Option<&str>) -> Option<String> {
    let declared = declared.unwrap_or("").trim();
    if !declared.is_empty() && !is_generic(declared) {
        let canon = canonical_mime(declared);
        if canon != declared.trim().to_ascii_lowercase() {
            return Some(canon);
        }
        return None;
    }
    let name = filename.unwrap_or("");
    let ext = name.rsplit('.').next().unwrap_or("");
    // A trailing dot or dot-file without extension must not invent a type.
    if ext.is_empty() || ext.len() == name.len() {
        return None;
    }
    mime_for_extension(ext).map(str::to_string)
}

/// File extension (no dot, lowercase) → MIME, the sender's `guess_mime`
/// table as a lookup. `None` for unknown extensions.
pub fn mime_for_extension(ext: &str) -> Option<&'static str> {
    Some(
        match ext
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "txt" | "log" | "md" | "markdown" | "text" => "text/plain",
            "html" | "htm" => "text/html",
            "csv" => "text/csv",
            "xml" => "application/xml",
            "json" => "application/json",
            "yaml" | "yml" => "application/yaml",
            "ics" | "ical" | "ifb" => "text/calendar",
            "vcs" => "text/x-vcalendar",
            "vcf" | "vcard" => "text/vcard",
            "eml" | "emlx" => "message/rfc822",
            "pdf" => "application/pdf",
            "rtf" => "application/rtf",
            "epub" => "application/epub+zip",
            "zip" => "application/zip",
            "7z" => "application/x-7z-compressed",
            "rar" => "application/vnd.rar",
            "tar" => "application/x-tar",
            "gz" | "tgz" => "application/gzip",
            "bz2" => "application/x-bzip2",
            "xz" => "application/x-xz",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "svg" | "svgz" => "image/svg+xml",
            "bmp" => "image/bmp",
            "ico" => "image/x-icon",
            "tif" | "tiff" => "image/tiff",
            "heic" | "heif" => "image/heic",
            "heics" => "image/heif-sequence",
            "avif" => "image/avif",
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "ogg" | "oga" => "audio/ogg",
            "opus" => "audio/opus",
            "flac" => "audio/flac",
            "aac" => "audio/aac",
            "m4a" => "audio/mp4",
            "mp4" | "m4v" => "video/mp4",
            "mov" => "video/quicktime",
            "webm" => "video/webm",
            "mkv" => "video/x-matroska",
            "avi" => "video/x-msvideo",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xls" => "application/vnd.ms-excel",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "ppt" => "application/vnd.ms-powerpoint",
            "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "odt" => "application/vnd.oasis.opendocument.text",
            "ods" => "application/vnd.oasis.opendocument.spreadsheet",
            "odp" => "application/vnd.oasis.opendocument.presentation",
            "apk" => "application/vnd.android.package-archive",
            _ => return None,
        },
    )
}
/// MIME → file extension (no dot), mirroring the sender's `guess_mime`
/// table the other way round.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    Some(match canonical_mime(mime).as_str() {
        "text/plain" => "txt",
        "text/html" => "html",
        "text/csv" => "csv",
        "application/xml" => "xml",
        "application/json" => "json",
        "application/yaml" => "yaml",
        "text/calendar" => "ics",
        "text/vcard" => "vcf",
        "message/rfc822" => "eml",
        "application/pdf" => "pdf",
        "application/rtf" => "rtf",
        "application/epub+zip" => "epub",
        "application/zip" => "zip",
        "application/x-7z-compressed" => "7z",
        "application/vnd.rar" => "rar",
        "application/x-tar" => "tar",
        "application/gzip" => "gz",
        "application/x-bzip2" => "bz2",
        "application/x-xz" => "xz",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/x-icon" => "ico",
        "image/tiff" => "tiff",
        "image/heic" => "heic",
        "image/heif" | "image/heif-sequence" => "heif",
        "image/avif" => "avif",
        "audio/mpeg" => "mp3",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        "audio/opus" => "opus",
        "audio/flac" => "flac",
        "audio/aac" => "aac",
        "audio/mp4" => "m4a",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/x-msvideo" => "avi",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.ms-powerpoint"
        | "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        "application/vnd.oasis.opendocument.text" => "odt",
        "application/vnd.oasis.opendocument.spreadsheet" => "ods",
        "application/vnd.oasis.opendocument.presentation" => "odp",
        "application/vnd.android.package-archive" => "apk",
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
    fn sniffing_covers_archives_media_and_text_shapes() {
        assert_eq!(sniff_mime(b"BM...."), Some("image/bmp"));
        assert_eq!(sniff_mime(b"\x00\x00\x01\x00.."), Some("image/x-icon"));
        assert_eq!(sniff_mime(b"II*\x00rest"), Some("image/tiff"));
        assert_eq!(sniff_mime(b"MM\x00*rest"), Some("image/tiff"));
        assert_eq!(
            sniff_mime(b"\x00\x00\x00\x18ftypheic\x00"),
            Some("image/heic")
        );
        assert_eq!(
            sniff_mime(b"\x00\x00\x00\x18ftypavif\x00"),
            Some("image/avif")
        );
        assert_eq!(
            sniff_mime(b"\x00\x00\x00\x18ftypisom\x00"),
            Some("video/mp4")
        );
        assert_eq!(
            sniff_mime(b"7z\xbc\xaf'\x1c.."),
            Some("application/x-7z-compressed")
        );
        assert_eq!(
            sniff_mime(b"Rar!\x1a\x07\x00.."),
            Some("application/vnd.rar")
        );
        assert_eq!(sniff_mime(b"\x1f\x8b\x08..."), Some("application/gzip"));
        assert_eq!(sniff_mime(b"ID3\x04..."), Some("audio/mpeg"));
        assert_eq!(sniff_mime(b"OggS..."), Some("audio/ogg"));
        assert_eq!(sniff_mime(b"fLaC..."), Some("audio/flac"));
        assert_eq!(
            sniff_mime(b"RIFF\x00\x00\x00\x00WAVEfmt "),
            Some("audio/wav")
        );
        assert_eq!(sniff_mime(b"\x1a\x45\xdf\xa3..."), Some("video/x-matroska"));
        assert_eq!(
            sniff_mime(b"BEGIN:VCALENDAR\r\nVERSION:2.0"),
            Some("text/calendar")
        );
        assert_eq!(sniff_mime(b"BEGIN:VCARD\r\nFN:x"), Some("text/vcard"));
        assert_eq!(
            sniff_mime(b"From: a@example.com\r\nTo: b@example.com\r\n\r\nx"),
            Some("message/rfc822")
        );
        // A BOM or blank line does not hide the shape.
        assert_eq!(
            sniff_mime(b"\xef\xbb\xbf\nBEGIN:VCALENDAR"),
            Some("text/calendar")
        );
    }

    #[test]
    fn zip_subtype_names_office_files() {
        let docx = b"PK\x03\x04....word/document.xml....";
        assert_eq!(
            sniff_mime(docx),
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        );
        // A generic header on docx bytes upgrades to the subtype.
        assert_eq!(
            corrected_mime(Some("application/octet-stream"), docx).as_deref(),
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
        );
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
        // Calendar bytes repair a generic header even without magic history.
        assert_eq!(
            corrected_mime(Some("application/octet-stream"), b"BEGIN:VCALENDAR").as_deref(),
            Some("text/calendar")
        );
    }

    #[test]
    fn calendar_aliases_canonicalize() {
        assert_eq!(canonical_mime("application/ics"), "text/calendar");
        assert_eq!(canonical_mime("TEXT/X-VCALENDAR"), "text/calendar");
        assert!(is_calendar_mime("application/ics"));
        assert!(!is_calendar_mime("text/plain"));
        assert_eq!(extension_for_mime("application/ics"), Some("ics"));
        assert_eq!(mime_for_extension("ics"), Some("text/calendar"));
        assert_eq!(
            repaired_mime(Some("application/ics"), Some("invite.ics")).as_deref(),
            Some("text/calendar")
        );
        assert_eq!(
            repaired_mime(Some("application/octet-stream"), Some("invite.ics")).as_deref(),
            Some("text/calendar")
        );
        assert_eq!(repaired_mime(Some("image/png"), Some("a.png")), None);
    }

    #[test]
    fn extensions_cover_unknown_exts() {
        assert_eq!(mime_for_extension("PDF"), Some("application/pdf"));
        assert_eq!(mime_for_extension("jpg"), Some("image/jpeg"));
        assert_eq!(mime_for_extension(".ICS"), Some("text/calendar"));
        assert_eq!(mime_for_extension("vcf"), Some("text/vcard"));
        assert_eq!(mime_for_extension("eml"), Some("message/rfc822"));
        assert_eq!(
            mime_for_extension("odt"),
            Some("application/vnd.oasis.opendocument.text")
        );
        assert_eq!(mime_for_extension("dat"), None);
        assert_eq!(mime_for_extension(""), None);
    }

    #[test]
    fn extensions_round_trip_the_sender_guesses() {
        assert_eq!(extension_for_mime("image/png"), Some("png"));
        assert_eq!(extension_for_mime("image/jpeg"), Some("jpg"));
        assert_eq!(extension_for_mime("IMAGE/JPG"), Some("jpg"));
        assert_eq!(extension_for_mime("application/pdf"), Some("pdf"));
        assert_eq!(extension_for_mime("application/ics"), Some("ics"));
        assert_eq!(extension_for_mime("text/x-vcard"), Some("vcf"));
        assert_eq!(extension_for_mime("message/rfc822"), Some("eml"));
        assert_eq!(extension_for_mime("application/octet-stream"), None);
    }
}
