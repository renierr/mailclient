//! Outgoing attachments: size/count caps, MIME guessing, and validated reads.

use crate::error::{Result, StoreError};

/// Max bytes per outgoing attachment (25 MiB, matches IMAP store cap).
pub const MAX_SEND_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;
/// Max files per outgoing message.
pub const MAX_SEND_ATTACHMENT_COUNT: usize = 20;
#[must_use]
pub fn guess_mime(filename: &str) -> String {
    let ext = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "txt" | "log" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "csv" => "text/csv",
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
        "ppt" | "pptx" => "application/vnd.ms-powerpoint",
        _ => "application/octet-stream",
    }
    .to_string()
}
/// Rejects missing files, directories, oversized files and over-count sends
/// with a human-readable error for the status bar.
pub fn load_outgoing_attachments(paths: &[String]) -> Result<Vec<(String, String, Vec<u8>)>> {
    if paths.len() > MAX_SEND_ATTACHMENT_COUNT {
        return Err(StoreError::InvalidInput(format!(
            "too many attachments ({} > {})",
            paths.len(),
            MAX_SEND_ATTACHMENT_COUNT
        )));
    }
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let raw = p.trim();
        // QML FileDialog hands `file://` URLs (percent-encoded,
        // `file:///C:/…` on Windows) — accept both URL and plain path.
        let path = crate::paths::file_url_to_path(raw);
        let meta = std::fs::metadata(&path).map_err(|_| {
            StoreError::InvalidInput(format!("cannot read attachment: {}", path.display()))
        })?;
        if !meta.is_file() {
            return Err(StoreError::InvalidInput(format!(
                "not a file: {}",
                path.display()
            )));
        }
        if meta.len() > MAX_SEND_ATTACHMENT_BYTES {
            return Err(StoreError::InvalidInput(format!(
                "{} is too large ({} MB > 25 MB)",
                path.display(),
                meta.len() / (1024 * 1024)
            )));
        }
        let bytes = std::fs::read(&path)?;
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin")
            .to_string();
        let mime = guess_mime(&filename);
        out.push((filename, mime, bytes));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_guess_covers_common_types() {
        assert_eq!(guess_mime("a.pdf"), "application/pdf");
        assert_eq!(guess_mime("photo.JPG"), "image/jpeg");
        assert_eq!(guess_mime("notes.txt"), "text/plain");
        assert_eq!(guess_mime("archive.unknownext"), "application/octet-stream");
        assert_eq!(guess_mime("noext"), "application/octet-stream");
    }

    #[test]
    fn outgoing_attachments_read_files_and_reject_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("hello.txt");
        std::fs::write(&file, b"hi").unwrap();
        // Plain path and file:// URL both work; mime comes from the extension.
        let loaded = load_outgoing_attachments(&[file.to_string_lossy().to_string()]).unwrap();
        assert_eq!(loaded[0].0, "hello.txt");
        assert_eq!(loaded[0].1, "text/plain");
        assert_eq!(loaded[0].2, b"hi");
        let url = format!("file://{}", file.display());
        assert!(load_outgoing_attachments(&[url]).is_ok());
        // Directories and missing files are user-facing errors, not panics.
        assert!(load_outgoing_attachments(&[dir.path().to_string_lossy().to_string()]).is_err());
        assert!(load_outgoing_attachments(&["/does/not/exist.bin".to_string()]).is_err());
    }
}
