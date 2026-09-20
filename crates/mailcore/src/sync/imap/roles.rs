//! Folder role mapping (SPECIAL-USE attributes + multilingual leaf heuristics),
//! selectability, path normalization, and CREATE-race detection.

use imap_types::flag::FlagNameAttribute;

use crate::error::{Result, StoreError};
use crate::models::FolderRole;

/// Format name attributes into lowercase string for heuristic search.
///
/// Uses `Display` (wire text like `\Noselect`) rather than `Debug`, so the
/// match does not depend on the `imap-types` `Debug` representation.
pub fn attr_text(attributes: &[FlagNameAttribute<'_>]) -> String {
    attributes
        .iter()
        .map(|a| a.to_string())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Whether a LISTED mailbox can hold messages (i.e. not `\Noselect` and not
/// a `\NonExistent` hierarchy placeholder).
#[must_use]
pub fn is_selectable(attributes: &[FlagNameAttribute<'_>]) -> bool {
    !attributes.iter().any(|a| {
        let s = a.to_string();
        s.eq_ignore_ascii_case("\\noselect") || s.eq_ignore_ascii_case("\\nonexistent")
    })
}

/// Map a LISTED mailbox to a [`FolderRole`].
///
/// Prefers RFC 6154 SPECIAL-USE attributes, falls back to multilingual name
/// heuristics, keeps everything else as `Custom` (user IMAP folders included).
#[must_use]
pub fn map_folder_role(attributes: &[FlagNameAttribute<'_>], name: &str) -> FolderRole {
    let attrs = attr_text(attributes);
    if attrs.contains("sent") {
        return FolderRole::Sent;
    }
    if attrs.contains("draft") {
        return FolderRole::Drafts;
    }
    if attrs.contains("trash") {
        return FolderRole::Trash;
    }
    if attrs.contains("junk") || attrs.contains("spam") {
        return FolderRole::Junk;
    }
    if attrs.contains("archive") {
        return FolderRole::Archive;
    }
    role_from_name(name)
}

/// Fallback role guessing based on the folder's leaf name.
///
/// Matches the last path segment exactly so names like `Cabin` / `Binders`
/// are not classified as Trash via a substring `bin`. Uses Unicode
/// `to_lowercase` so capitalized umlauts (`Entwürfe`, `Gelöschte …`) match.
/// Covers both the pre-`imap-next` variant table and the newer
/// `gesendete …` / `deleted messages` / `bulk mail` additions.
#[must_use]
pub fn role_from_name(name: &str) -> FolderRole {
    let lower = name.to_lowercase();
    let leaf = lower
        .rsplit(['/', '.', '\\'])
        .next()
        .unwrap_or(&lower)
        .trim();

    if leaf == "inbox" {
        return FolderRole::Inbox;
    }
    match leaf {
        "sent" | "sent mail" | "sent-mail" | "sent items" | "sent messages" | "gesendet"
        | "gesendete elemente" | "gesendete objekte" => FolderRole::Sent,
        "draft" | "drafts" | "entwurf" | "entwürfe" | "entwurfe" | "entwuerfe" => {
            FolderRole::Drafts
        }
        "trash"
        | "deleted"
        | "deleted items"
        | "deleted messages"
        | "papierkorb"
        | "gelöscht"
        | "geloscht"
        | "gelöschte elemente"
        | "geloeschte elemente"
        | "bin" => FolderRole::Trash,
        "junk" | "junk mail" | "junk e-mail" | "junk email" | "junk-e-mail" | "spam"
        | "bulk mail" | "unerwünscht" => FolderRole::Junk,
        "archive" | "archiv" => FolderRole::Archive,
        _ => FolderRole::Custom,
    }
}
/// Best-effort "mailbox already exists" detection for CREATE races: servers
/// word it differently (`ALREADYEXISTS`, `already exists`, `exists`), so a
/// case-insensitive substring match beats an exact one.
pub(crate) fn is_already_exists(e: &StoreError) -> bool {
    let msg = e.to_string().to_ascii_lowercase();
    msg.contains("already exists") || msg.contains("alreadyexists") || msg.contains("exists")
}
/// Validate + normalize a user-typed folder path: trims whitespace, maps `/`
/// separators onto the account's hierarchy `delimiter`, rejects empties,
/// empty segments (`a//b`), and the LIST wildcards `*`/`%` (legal in theory,
/// but they would corrupt our own subtree discovery patterns).
pub fn normalize_folder_path(input: &str, delimiter: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(StoreError::InvalidInput("folder name is empty".to_string()));
    }
    if trimmed.contains('*') || trimmed.contains('%') {
        return Err(StoreError::InvalidInput(
            "folder names may not contain * or %".to_string(),
        ));
    }
    let unified = if delimiter != "/" {
        trimmed.replace('/', delimiter)
    } else {
        trimmed.to_string()
    };
    let segments: Vec<&str> = unified.split(delimiter).map(str::trim).collect();
    if segments.iter().any(|s| s.is_empty()) {
        return Err(StoreError::InvalidInput(
            "folder names may not be empty or contain empty levels".to_string(),
        ));
    }
    if segments.iter().any(|s| s.chars().any(char::is_control)) {
        return Err(StoreError::InvalidInput(
            "folder names may not contain control characters".to_string(),
        ));
    }
    Ok(segments.join(delimiter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_path_normalization() {
        assert_eq!(normalize_folder_path("  INBOX  ", "/").unwrap(), "INBOX");
        assert_eq!(
            normalize_folder_path("INBOX/Archive/2026", "/").unwrap(),
            "INBOX/Archive/2026"
        );
        assert_eq!(
            normalize_folder_path("  INBOX . Archive . 2026  ", ".").unwrap(),
            "INBOX.Archive.2026"
        );
    }

    #[test]
    fn role_heuristics_cover_german_and_english_names() {
        assert_eq!(role_from_name("INBOX"), FolderRole::Inbox);
        assert_eq!(role_from_name("Sent"), FolderRole::Sent);
        assert_eq!(role_from_name("Gesendete Elemente"), FolderRole::Sent);
        assert_eq!(role_from_name("Entwürfe"), FolderRole::Drafts);
        assert_eq!(role_from_name("Papierkorb"), FolderRole::Trash);
        assert_eq!(role_from_name("Gelöschte Elemente"), FolderRole::Trash);
        assert_eq!(role_from_name("Spam"), FolderRole::Junk);
        assert_eq!(role_from_name("Archiv"), FolderRole::Archive);
    }

    #[test]
    fn role_from_name_keeps_legacy_variants_and_unicode() {
        // Legacy table (pre-rewrite) must keep working.
        assert_eq!(role_from_name("INBOX"), FolderRole::Inbox);
        assert_eq!(role_from_name("Sent"), FolderRole::Sent);
        assert_eq!(role_from_name("Sent Mail"), FolderRole::Sent);
        assert_eq!(role_from_name("Sent-Mail"), FolderRole::Sent);
        assert_eq!(role_from_name("Entwurf"), FolderRole::Drafts);
        assert_eq!(role_from_name("Entwurfe"), FolderRole::Drafts);
        assert_eq!(role_from_name("Gelöscht"), FolderRole::Trash);
        assert_eq!(role_from_name("Geloscht"), FolderRole::Trash);
        assert_eq!(role_from_name("Junk E-Mail"), FolderRole::Junk);
        assert_eq!(role_from_name("Junk-E-Mail"), FolderRole::Junk);
        // Unicode capitals (to_lowercase, not to_ascii_lowercase).
        assert_eq!(role_from_name("Entwürfe"), FolderRole::Drafts);
        assert_eq!(role_from_name("Gelöschte Elemente"), FolderRole::Trash);
        assert_eq!(role_from_name("Gesendete Elemente"), FolderRole::Sent);
        // New additions keep working too.
        assert_eq!(role_from_name("Sent Messages"), FolderRole::Sent);
        assert_eq!(role_from_name("Deleted Messages"), FolderRole::Trash);
        assert_eq!(role_from_name("Bulk Mail"), FolderRole::Junk);
        // Exact leaf match: no substring false-positives.
        assert_eq!(role_from_name("Cabin"), FolderRole::Custom);
        assert_eq!(role_from_name("Binders"), FolderRole::Custom);
        assert_eq!(role_from_name("INBOX.Archive"), FolderRole::Archive);
    }

    #[test]
    fn normalize_folder_path_rejects_wildcards_and_empty_levels() {
        assert_eq!(normalize_folder_path("  INBOX  ", "/").unwrap(), "INBOX");
        assert_eq!(
            normalize_folder_path("Work/Client", "/").unwrap(),
            "Work/Client"
        );
        assert!(normalize_folder_path("", "/").is_err());
        assert!(normalize_folder_path("   ", "/").is_err());
        assert!(normalize_folder_path("a//b", "/").is_err());
        assert!(normalize_folder_path("/Lead", "/").is_err());
        assert!(normalize_folder_path("a*b", "/").is_err());
        assert!(normalize_folder_path("a%b", "/").is_err());
        // `/` maps onto a dotted delimiter.
        assert_eq!(
            normalize_folder_path("Work/Client", ".").unwrap(),
            "Work.Client"
        );
    }

    #[test]
    fn selectable_and_special_use_mapping() {
        use imap_types::flag::FlagNameAttribute;
        assert!(is_selectable(&[]));
        assert!(!is_selectable(&[FlagNameAttribute::Noselect]));
        // Unknown server attributes arrive as Extension; `\Nonexistent`
        // placeholders must still be skipped.
        let nonexistent =
            FlagNameAttribute::from(imap_types::core::Atom::try_from("Nonexistent").unwrap());
        assert!(!is_selectable(&[nonexistent]));
        // attr_text uses Display wire text, not Debug internals.
        assert!(attr_text(&[FlagNameAttribute::Noselect]).contains("noselect"));
        assert_eq!(
            map_folder_role(&[FlagNameAttribute::Marked], "MyFolder"),
            FolderRole::Custom
        );
    }
}
