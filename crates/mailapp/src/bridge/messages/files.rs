//! Turning dialog values and mail-supplied names into paths we may write.
//!
//! Both directions are hostile in their own way: a QML dialog hands back a
//! percent-encoded `file://` URL whose shape differs per platform, and an
//! attachment's filename comes from the mail, so it is attacker-chosen.

use mailcore::store::messages;

/// Turn save-dialog output into a plain path. Dialogs hand back `file://`
/// URLs (percent-encoded, `file:///C:/…` on Windows); plain paths pass
/// through. Delegates to the shared `mailcore::paths` helper so save, open
/// and composer-send all parse URLs identically.
pub(crate) fn dir_to_path(raw: &str) -> std::path::PathBuf {
    mailcore::paths::file_url_to_path(raw)
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

/// Windows treats these as device names in any directory, with or without an
/// extension: opening `CON` or `LPT1.txt` for writing talks to the device
/// instead of creating a file.
const WINDOWS_DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Filename safe for the filesystem: keeps the basename, falls back to
/// `attachment-<id>.bin`.
///
/// The name comes from the mail, so it is attacker-chosen and gets checked
/// rather than trusted:
///
/// - only the basename survives, and the separators are replaced, so nothing
///   can steer the write out of the directory the user picked;
/// - `:` goes too — on NTFS `report.pdf:payload` writes an alternate data
///   stream hidden behind an innocuous name, and the other reserved Windows
///   characters would simply fail the write;
/// - `.` and `..` are not names, they are the directory itself and its
///   parent;
/// - a Windows device name (`CON`, `LPT1`, …) is prefixed, because opening it
///   reaches the device, not a file;
/// - trailing dots and spaces are stripped, which Windows does silently
///   anyway — leaving them would make the saved file's name differ from the
///   one reported back to the user.
///
/// The rules are applied on every platform: an attachment saved on Linux can
/// land on a shared or FAT/NTFS volume, and consistent names are easier to
/// reason about than per-OS ones.
pub(crate) fn safe_filename(name: Option<&str>, id: i64) -> String {
    let fallback = || format!("attachment-{id}.bin");
    let base = name
        .unwrap_or("")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    let cleaned = base
        .replace(['/', '\\', '\0', ':', '*', '?', '"', '<', '>', '|'], "_")
        .trim_end_matches([' ', '.'])
        .to_string();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        return fallback();
    }
    let stem = cleaned.split('.').next().unwrap_or("");
    if WINDOWS_DEVICE_NAMES
        .iter()
        .any(|d| stem.eq_ignore_ascii_case(d))
    {
        return format!("_{cleaned}");
    }
    cleaned
}

/// `photo.pdf` + 1 → `photo(1).pdf` (save-all collision avoidance).
pub(crate) fn numbered_filename(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{}({n}).{}", &name[..i], &name[i + 1..]),
        _ => format!("{name}({n})"),
    }
}

/// Resolve the save-dialog target for one attachment: `file://` tolerant,
/// directories auto-append the attachment filename, parents created.
pub(crate) fn resolve_save_path(
    db: &mailcore::Db,
    attachment_id: i64,
    raw: &str,
) -> Result<std::path::PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("choose where to save".to_string());
    }
    let mut p = dir_to_path(trimmed);
    if p.is_dir() || trimmed.ends_with('/') || trimmed.ends_with('\\') {
        let a = messages::get_attachment(db, attachment_id).map_err(|e| e.to_string())?;
        p.push(safe_filename(a.filename.as_deref(), attachment_id));
    }
    if let Some(parent) = p.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create folder: {e}"))?;
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::{numbered_filename, safe_filename};

    #[test]
    fn safe_filename_keeps_ordinary_names() {
        assert_eq!(safe_filename(Some("report.pdf"), 1), "report.pdf");
        assert_eq!(safe_filename(Some("  spaced.txt  "), 1), "spaced.txt");
        assert_eq!(safe_filename(Some("Ümläut ök.pdf"), 1), "Ümläut ök.pdf");
    }

    #[test]
    fn safe_filename_refuses_to_leave_the_chosen_directory() {
        // Only the basename survives, so no traversal and no absolute path.
        assert_eq!(safe_filename(Some("../../etc/passwd"), 1), "passwd");
        assert_eq!(safe_filename(Some("..\\..\\windows\\x.dll"), 1), "x.dll");
        assert_eq!(safe_filename(Some("/etc/passwd"), 1), "passwd");
        // `.` and `..` name directories, not files.
        assert_eq!(safe_filename(Some(".."), 7), "attachment-7.bin");
        assert_eq!(safe_filename(Some("."), 7), "attachment-7.bin");
    }

    #[test]
    fn safe_filename_defuses_windows_traps() {
        // NTFS alternate data stream hidden behind an innocuous name.
        assert_eq!(
            safe_filename(Some("report.pdf:payload"), 1),
            "report.pdf_payload"
        );
        // Device names reach the device, not a file — extension or not.
        assert_eq!(safe_filename(Some("CON"), 1), "_CON");
        assert_eq!(safe_filename(Some("lpt1.txt"), 1), "_lpt1.txt");
        // …but only the exact names.
        assert_eq!(safe_filename(Some("console.log"), 1), "console.log");
        // Windows strips these silently; do it here so the saved name is the
        // name the user is told about.
        assert_eq!(safe_filename(Some("trailing. . "), 1), "trailing");
        // Characters Windows reserves outright.
        assert_eq!(safe_filename(Some("a*b?c|d.txt"), 1), "a_b_c_d.txt");
    }

    #[test]
    fn safe_filename_falls_back_when_nothing_usable_is_left() {
        assert_eq!(safe_filename(None, 42), "attachment-42.bin");
        assert_eq!(safe_filename(Some(""), 42), "attachment-42.bin");
        assert_eq!(safe_filename(Some("   "), 42), "attachment-42.bin");
        assert_eq!(safe_filename(Some("dir/"), 42), "attachment-42.bin");
    }

    #[test]
    fn numbered_filename_keeps_the_extension() {
        assert_eq!(numbered_filename("photo.pdf", 1), "photo(1).pdf");
        assert_eq!(numbered_filename("archive.tar.gz", 2), "archive.tar(2).gz");
        assert_eq!(numbered_filename("noext", 3), "noext(3)");
        assert_eq!(numbered_filename(".hidden", 4), ".hidden(4)");
    }
}
