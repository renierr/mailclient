//! File-URL handling shared by every path that crosses the QML bridge, and
//! the names attachments are written under.
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

/// Windows treats these as device names in any directory, with or without an
/// extension: opening `CON` or `LPT1.txt` for writing talks to the device
/// instead of creating a file.
const WINDOWS_DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// What an attachment is called when the mail names it nothing usable.
pub fn fallback_attachment_name(id: i64) -> String {
    format!("attachment-{id}.bin")
}

/// Filename safe for the filesystem: keeps the basename, falls back to
/// [`fallback_attachment_name`]. Used for every write of an attachment, by
/// both frontends.
///
/// The name comes from the mail, so it is attacker-chosen and gets checked
/// rather than trusted:
///
/// - only the basename survives, and the separators are replaced, so nothing
///   can steer the write out of the directory the user picked;
/// - `:` goes too — on NTFS `report.pdf:payload` writes an alternate data
///   stream hidden behind an innocuous name — as do control characters and
///   the other reserved Windows characters, which would fail the write;
/// - `.` and `..` are not names, they are the directory itself and its
///   parent;
/// - a Windows device name (`CON`, `LPT1`, …) is prefixed, because opening it
///   reaches the device, not a file;
/// - trailing dots and spaces are stripped, which Windows does silently
///   anyway — leaving them would make the saved file's name differ from the
///   one reported back to the user.
///
/// The rules are applied on every platform: an attachment saved on Linux or
/// Android can land on a shared or FAT/NTFS volume, and consistent names are
/// easier to reason about than per-OS ones.
pub fn safe_attachment_name(name: Option<&str>, id: i64) -> String {
    let base = name
        .unwrap_or("")
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim();
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_end_matches([' ', '.']);
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.') {
        return fallback_attachment_name(id);
    }
    let stem = cleaned.split('.').next().unwrap_or("");
    if WINDOWS_DEVICE_NAMES
        .iter()
        .any(|d| stem.eq_ignore_ascii_case(d))
    {
        return format!("_{cleaned}");
    }
    cleaned.to_string()
}

/// `photo.pdf` + 1 → `photo(1).pdf`, for saving next to a same-named file.
pub fn numbered_filename(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{}({n}).{}", &name[..i], &name[i + 1..]),
        _ => format!("{name}({n})"),
    }
}

/// `dir/name`, or the first `dir/name(n)` that does not exist yet.
pub fn free_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let mut p = dir.join(name);
    let mut n = 1;
    while p.exists() {
        p = dir.join(numbered_filename(name, n));
        n += 1;
    }
    p
}

/// How long a viewer copy or draft staging dir may lie around in the
/// platform temp folder before it is pruned (24 h). The external viewer
/// keeps its own handle once opened, so deleting a stale copy only stops
/// storage from filling up — on Android this is the app cache, which the
/// OS otherwise clears on its own schedule only.
pub const TEMP_COPY_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// Prefix of the per-draft staging dirs created for the composer.
pub const DRAFT_TEMP_PREFIX: &str = "mailclient-draft-";

/// Best-effort prune of viewer copies older than [`TEMP_COPY_MAX_AGE`] in
/// `dir` (the dedicated `mailclient-attachments` folder). `keep` is the
/// file just written and is never deleted. Never fails — a prune must not
/// break the open it runs alongside.
pub fn prune_temp_copies(dir: &std::path::Path, keep: Option<&std::path::Path>) {
    prune_older_than(dir, TEMP_COPY_MAX_AGE, keep);
}

/// Best-effort prune of stale [`DRAFT_TEMP_PREFIX`] staging dirs in the
/// base temp folder. Never fails, for the same reason as above.
pub fn prune_stale_draft_dirs(base: &std::path::Path) {
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return,
    };
    let cutoff = std::time::SystemTime::now()
        .checked_sub(TEMP_COPY_MAX_AGE)
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(DRAFT_TEMP_PREFIX) {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let stale = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| t <= cutoff)
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

fn prune_older_than(
    dir: &std::path::Path,
    max_age: std::time::Duration,
    keep: Option<&std::path::Path>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let cutoff = std::time::SystemTime::now()
        .checked_sub(max_age)
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(k) = keep {
            if path == k {
                continue;
            }
        }
        if !path.is_file() {
            continue;
        }
        let stale = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .map(|t| t <= cutoff)
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_name_keeps_ordinary_names() {
        assert_eq!(safe_attachment_name(Some("report.pdf"), 1), "report.pdf");
        assert_eq!(
            safe_attachment_name(Some("  spaced.txt  "), 1),
            "spaced.txt"
        );
        assert_eq!(
            safe_attachment_name(Some("Ümläut ök.pdf"), 1),
            "Ümläut ök.pdf"
        );
    }

    #[test]
    fn safe_name_refuses_to_leave_the_chosen_directory() {
        // Only the basename survives, so no traversal and no absolute path.
        assert_eq!(safe_attachment_name(Some("../../etc/passwd"), 1), "passwd");
        assert_eq!(
            safe_attachment_name(Some("..\\..\\windows\\x.dll"), 1),
            "x.dll"
        );
        assert_eq!(safe_attachment_name(Some("/etc/passwd"), 1), "passwd");
        // A dot-file keeps its leading dot: it stays inside the folder.
        assert_eq!(safe_attachment_name(Some("../../.bashrc"), 1), ".bashrc");
        // `.` and `..` name directories, not files.
        assert_eq!(safe_attachment_name(Some(".."), 7), "attachment-7.bin");
        assert_eq!(safe_attachment_name(Some("."), 7), "attachment-7.bin");
    }

    #[test]
    fn safe_name_defuses_windows_traps() {
        // NTFS alternate data stream hidden behind an innocuous name.
        assert_eq!(
            safe_attachment_name(Some("report.pdf:payload"), 1),
            "report.pdf_payload"
        );
        // Device names reach the device, not a file — extension or not.
        assert_eq!(safe_attachment_name(Some("CON"), 1), "_CON");
        assert_eq!(safe_attachment_name(Some("lpt1.txt"), 1), "_lpt1.txt");
        // …but only the exact names.
        assert_eq!(safe_attachment_name(Some("console.log"), 1), "console.log");
        // Windows strips these silently; do it here so the saved name is the
        // name the user is told about.
        assert_eq!(safe_attachment_name(Some("trailing. . "), 1), "trailing");
        // Characters Windows reserves outright, and control characters.
        assert_eq!(safe_attachment_name(Some("a*b?c|d.txt"), 1), "a_b_c_d.txt");
        assert_eq!(safe_attachment_name(Some("a\0b\tc.txt"), 1), "a_b_c.txt");
    }

    #[test]
    fn safe_name_falls_back_when_nothing_usable_is_left() {
        assert_eq!(safe_attachment_name(None, 42), "attachment-42.bin");
        assert_eq!(safe_attachment_name(Some(""), 42), "attachment-42.bin");
        assert_eq!(safe_attachment_name(Some("   "), 42), "attachment-42.bin");
        assert_eq!(safe_attachment_name(Some("dir/"), 42), "attachment-42.bin");
    }

    #[test]
    fn numbered_filename_keeps_the_extension() {
        assert_eq!(numbered_filename("photo.pdf", 1), "photo(1).pdf");
        assert_eq!(numbered_filename("archive.tar.gz", 2), "archive.tar(2).gz");
        assert_eq!(numbered_filename("noext", 3), "noext(3)");
        assert_eq!(numbered_filename(".hidden", 4), ".hidden(4)");
    }

    #[test]
    fn free_path_numbers_around_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(free_path(dir.path(), "a.txt"), dir.path().join("a.txt"));
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        std::fs::write(dir.path().join("a(1).txt"), b"x").unwrap();
        assert_eq!(free_path(dir.path(), "a.txt"), dir.path().join("a(2).txt"));
    }

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

    #[cfg(unix)]
    fn age(path: &std::path::Path, secs: u64) {
        use std::time::{Duration, SystemTime};
        let old = SystemTime::now() - Duration::from_secs(secs);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(old)
            .unwrap();
    }

    #[test]
    fn prune_keeps_fresh_copies_and_drops_stale_ones() {
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("1-1-fresh.pdf");
        let stale = dir.path().join("2-2-stale.pdf");
        std::fs::write(&fresh, b"x").unwrap();
        std::fs::write(&stale, b"x").unwrap();
        #[cfg(unix)]
        age(&stale, 7 * 24 * 3600);
        #[cfg(unix)]
        {
            prune_temp_copies(dir.path(), None);
            assert!(fresh.exists());
            assert!(!stale.exists());
        }
        #[cfg(not(unix))]
        {
            // Without mtime control both are fresh and survive.
            prune_temp_copies(dir.path(), None);
            assert!(fresh.exists());
            assert!(stale.exists());
        }
    }

    #[test]
    fn prune_never_deletes_the_file_just_written() {
        let dir = tempfile::tempdir().unwrap();
        let keep = dir.path().join("3-3-keep.pdf");
        std::fs::write(&keep, b"x").unwrap();
        prune_older_than(&keep, std::time::Duration::ZERO, Some(&keep));
        assert!(keep.exists());
    }

    #[test]
    fn prune_tolerates_a_missing_folder() {
        let dir = tempfile::tempdir().unwrap();
        prune_temp_copies(&dir.path().join("gone"), None);
        prune_stale_draft_dirs(&dir.path().join("gone"));
    }

    #[test]
    fn prune_leaves_unrelated_files_and_dirs_alone() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("something-else");
        std::fs::create_dir(&other).unwrap();
        let plain = dir.path().join("notes.txt");
        std::fs::write(&plain, b"x").unwrap();
        prune_stale_draft_dirs(dir.path());
        assert!(other.exists());
        assert!(plain.exists());
    }
}
