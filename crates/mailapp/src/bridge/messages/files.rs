//! Paths to `file://` URLs for QML. Reading dialog values, naming and
//! writing attachments is `mailcore`'s (`paths`, `store::messages::save_*`),
//! shared with the Flutter adapter.

/// `file://` URL for `name` inside `dir`, for the save dialogs' preset.
/// `dir` is whatever QML holds: `StandardPaths` gives a QUrl on Qt 6 and a
/// plain path elsewhere, and both are accepted. An empty `name` gives the
/// folder itself. Replaces a QML twin of this URL building (D11).
pub(crate) fn file_url_in(dir: &str, name: &str) -> String {
    let mut path = mailcore::paths::file_url_to_path(dir);
    if !name.is_empty() {
        path.push(name);
    }
    file_url(&path)
}

/// The decoded file name a dialog URL or path ends in; the input itself when
/// it names none. Replaces a QML twin of the decoding (D11).
pub(crate) fn file_name_of(url: &str) -> String {
    mailcore::paths::file_url_to_path(url)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| url.to_string())
}

/// Absolute path → `file://` URL for `Qt.openUrlExternally`. Percent-encodes
/// everything but `/` and unreserved characters so spaces, `#` and non-ASCII
/// names survive the QML string→QUrl conversion.
pub(crate) fn file_url(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::with_capacity(s.len() + 7);
    out.push_str("file://");
    // A bare `C:/…` would parse as host `C:` — anchor it as an empty host.
    if !(s.starts_with('/') || s.starts_with("file:")) {
        out.push('/');
    }
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_url_or_path_joins_an_encoded_name() {
        // `StandardPaths` hands QML a QUrl on Qt 6 and a plain path elsewhere.
        for dir in ["file:///home/a", "/home/a", "file:///home/a/"] {
            assert_eq!(
                file_url_in(dir, "x y#1.pdf"),
                "file:///home/a/x%20y%231.pdf",
                "{dir}"
            );
        }
        assert_eq!(file_url_in("file:///home/a", ""), "file:///home/a");
    }

    #[test]
    fn the_name_of_a_dialog_url_is_decoded() {
        assert_eq!(file_name_of("file:///home/a/x%20y.pdf"), "x y.pdf");
        assert_eq!(file_name_of("/home/a/plain%20name.txt"), "plain%20name.txt");
        assert_eq!(file_name_of("file:///"), "file:///");
    }
}
