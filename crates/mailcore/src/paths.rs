//! File-URL handling shared by every path that crosses the QML bridge.
//!
//! QML file/folder dialogs hand back `file://` URLs (`selectedFile` /
//! `selectedFolder` / composer `selectedFiles`), never plain paths. Three
//! things go wrong when those are turned into paths by merely stripping the
//! `file://` prefix:
//!
//! - On Windows the dialog returns `file:///C:/Users/…` (three slashes).
//!   Stripping `file://` leaves `/C:/Users/…`, which is not a valid drive
//!   path — `create_dir_all` fails with OS error 123, so every attachment
//!   save fails. The leading slash must go when it precedes a drive letter.
//! - `QUrl::toString()` percent-encodes (`a b.pdf` → `a%20b.pdf`, `#` →
//!   `%23`). Without decoding, saves create `%20` files or miss entirely.
//! - Old QML built `file://C:/…` (host form); both shapes must be accepted,
//!   plus `file://server/share/…` UNC and plain (non-URL) paths.

use std::path::PathBuf;

/// Percent-decode `%XX` sequences. Invalid sequences survive verbatim; `+`
/// is a literal plus (paths, not query strings).
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Does `s` look like `/C:` / `/C:/…` — a drive letter with a stray leading
/// slash from `file:///C:/…`?
fn has_drive_prefix(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b.get(2) == Some(&b':')
}

/// Turn a save/composer dialog value into a filesystem path.
///
/// Accepts `file://` URLs (any of `file:///C:/…`, `file://C:/…`,
/// `file:///home/…`, `file://server/share/…`, `file://localhost/…`) and
/// plain paths. URL forms are percent-decoded; plain paths pass through
/// untouched (a literal `%20` in a real filename must survive).
pub fn file_url_to_path(raw: &str) -> PathBuf {
    let t = raw.trim();
    let Some(after_scheme) = t.strip_prefix("file:") else {
        return PathBuf::from(t);
    };
    // `file:` without `//` (e.g. `file:/home/a`) — treat the rest as path.
    let mut rest = after_scheme.strip_prefix("//").unwrap_or(after_scheme);
    // `file://localhost/home/a` → `/home/a`.
    rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest);
    // `file:///C:/…` → `/C:/…` → `C:/…`; `file:///home/…` keeps its `/`.
    let fixed = if has_drive_prefix(&decoded) {
        decoded[1..].to_string()
    } else {
        decoded
    };
    // `file://server/share/…` (no leading slash, no drive): UNC on Windows,
    // `//server/share/…` elsewhere — both parse back to the same file.
    if !fixed.starts_with('/') && !fixed.starts_with("\\\\") && fixed.contains('/') {
        let is_drive = fixed.len() >= 2
            && fixed.as_bytes()[1] == b':'
            && fixed.as_bytes()[0].is_ascii_alphabetic();
        if !is_drive {
            #[cfg(windows)]
            return PathBuf::from(format!("\\\\{}", fixed.replace('/', "\\")));
            #[cfg(not(windows))]
            return PathBuf::from(format!("/{fixed}"));
        }
    }
    PathBuf::from(fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_three_slash_url_strips_leading_slash() {
        assert_eq!(
            file_url_to_path("file:///C:/Users/u/Downloads/report.pdf"),
            PathBuf::from("C:/Users/u/Downloads/report.pdf")
        );
    }

    #[test]
    fn windows_host_form_url_kept_as_drive_path() {
        // What the old QML `file:// + C:/…` concatenation produced.
        assert_eq!(
            file_url_to_path("file://C:/Users/u/Downloads/report.pdf"),
            PathBuf::from("C:/Users/u/Downloads/report.pdf")
        );
    }

    #[test]
    fn posix_url_keeps_leading_slash() {
        assert_eq!(
            file_url_to_path("file:///home/u/Downloads/x.pdf"),
            PathBuf::from("/home/u/Downloads/x.pdf")
        );
    }

    #[test]
    fn url_percent_decoding() {
        assert_eq!(
            file_url_to_path("file:///home/u/Downloads/a%20b%23c.pdf"),
            PathBuf::from("/home/u/Downloads/a b#c.pdf")
        );
        assert_eq!(
            file_url_to_path("file:///C:/Users/u/My%20Docs/a%20b.pdf"),
            PathBuf::from("C:/Users/u/My Docs/a b.pdf")
        );
    }

    #[test]
    fn plain_paths_pass_through_verbatim() {
        assert_eq!(
            file_url_to_path("C:/Users/u/Downloads/report.pdf"),
            PathBuf::from("C:/Users/u/Downloads/report.pdf")
        );
        assert_eq!(
            file_url_to_path("/home/u/Downloads/x.pdf"),
            PathBuf::from("/home/u/Downloads/x.pdf")
        );
        // A literal `%20` in a real filename is not decoded for plain paths.
        assert_eq!(
            file_url_to_path("/home/u/a%20b.pdf"),
            PathBuf::from("/home/u/a%20b.pdf")
        );
    }

    #[test]
    fn localhost_prefix_stripped() {
        assert_eq!(
            file_url_to_path("file://localhost/home/u/x.pdf"),
            PathBuf::from("/home/u/x.pdf")
        );
    }
}
