//! Paths to `file://` URLs for QML. Reading dialog values, naming and
//! writing attachments is `mailcore`'s (`paths`, `store::messages::save_*`),
//! shared with the Flutter adapter.

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
